use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::support::insights::{self, OutputObservation, TextMetrics};
use crate::support::paths::global_cache_root;
use crate::support::runner::{
    append_failure_hint, capture, ProxyOutcome, FAILURE_ARTIFACT_EXPANSION_REASON,
};
use crate::support::utils::resolved_command;

pub fn run_diff(args: &[String]) -> Result<ProxyOutcome> {
    let optimizations_enabled = insights::command_optimizations_enabled().unwrap_or(true);
    if optimizations_enabled {
        if let Some(materialized) = materialize_no_index_fd_args(args)? {
            return run_diff_with_materialized_args(args, materialized);
        }
    }
    let wants_direct_output = args.iter().any(|arg| is_direct_diff_arg(arg));
    let wants_compact = !args.iter().any(|arg| arg == "--no-compact");
    if wants_direct_output || !wants_compact || !optimizations_enabled {
        return run_direct_diff(args);
    }

    run_compact_diff(args)
}

fn is_direct_diff_arg(arg: &str) -> bool {
    matches!(
        arg,
        "--stat"
            | "--numstat"
            | "--shortstat"
            | "--summary"
            | "--name-only"
            | "--name-status"
            | "--raw"
            | "--check"
            | "--quiet"
            | "--exit-code"
    ) || arg.starts_with("--stat=")
        || arg.starts_with("--dirstat")
}

fn run_direct_diff(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("git");
    cmd.arg("diff");
    for arg in args {
        if arg != "--no-compact" {
            cmd.arg(arg);
        }
    }
    let mut output = capture(cmd, "git diff")?;
    let exit_code = output.exit_code;
    let failure_hint = if exit_code == 0
        || is_no_index_difference_exit(args, exit_code, &output.stdout, &output.stderr)
    {
        None
    } else {
        output.failure_artifact_hint("git")
    };
    let observation = output.observation("git diff");
    Ok(ProxyOutcome {
        stdout: append_failure_hint(
            output.stdout.trim_end().to_string(),
            failure_hint.as_deref(),
        ),
        stderr: output.stderr.trim_end().to_string(),
        exit_code,
        observation: None,
    }
    .with_observation(observation))
}

fn run_compact_diff(args: &[String]) -> Result<ProxyOutcome> {
    let mut stat_cmd = resolved_command("git");
    stat_cmd.arg("diff").arg("--stat").args(args);
    let mut stat_output = capture(stat_cmd, "git diff --stat")?;
    let exit_code = stat_output.exit_code;
    if exit_code != 0
        && !is_no_index_difference_exit(args, exit_code, &stat_output.stdout, &stat_output.stderr)
    {
        let failure_hint = stat_output.failure_artifact_hint("git");
        let observation = stat_output.observation("git diff --stat");
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(stat_output.stdout, failure_hint.as_deref()),
            stderr: stat_output.stderr.trim_end().to_string(),
            exit_code,
            observation: None,
        }
        .with_observation(observation));
    }

    let stat_stdout = stat_output.stdout;

    let mut diff_cmd = resolved_command("git");
    diff_cmd.arg("diff").args(args);
    let mut diff_output = capture(diff_cmd, "git diff")?;
    let diff_observation = diff_output.observation("git diff");
    let diff_exit_code = diff_output.exit_code;
    let diff_combined = diff_output.combined.clone();
    let raw_output = if stat_stdout.trim().is_empty() {
        diff_combined.clone()
    } else if diff_combined.trim().is_empty() {
        stat_stdout.clone()
    } else {
        format!("{stat_stdout}\n{diff_combined}")
    };

    let mut stdout = stat_stdout.trim_end().to_string();
    if !diff_output.stdout.trim().is_empty() {
        stdout.push_str("\n\n--- Changes ---\n");
        stdout.push_str(&compact_diff(&diff_output.stdout, 500));
    }
    if diff_exit_code != 0
        && !is_no_index_difference_exit(
            args,
            diff_exit_code,
            &diff_output.stdout,
            &diff_output.stderr,
        )
    {
        let failure_hint =
            diff_output.failure_artifact_hint_with_stdout_prefix("git", &stat_stdout);
        let failure_observation = OutputObservation::from_metrics_with_response(
            "git diff",
            TextMetrics::from_text(&stat_stdout).plus(diff_observation.metrics),
            &raw_output,
        );
        let failure_observation = if failure_hint.is_some() {
            failure_observation.with_expansion_reason(FAILURE_ARTIFACT_EXPANSION_REASON)
        } else {
            failure_observation
        };
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(stdout, failure_hint.as_deref()),
            stderr: diff_output.stderr.trim_end().to_string(),
            exit_code: diff_exit_code,
            observation: None,
        }
        .with_observation(failure_observation));
    }

    Ok(ProxyOutcome {
        stdout,
        stderr: String::new(),
        exit_code: diff_exit_code,
        observation: None,
    }
    .with_raw_output("git diff", &raw_output))
}

pub(super) struct MaterializedNoIndexArgs {
    pub(super) args: Vec<String>,
    files: Vec<PathBuf>,
}

impl Drop for MaterializedNoIndexArgs {
    fn drop(&mut self) {
        for path in &self.files {
            let _ = fs::remove_file(path);
        }
    }
}

fn run_diff_with_materialized_args(
    _original_args: &[String],
    materialized: MaterializedNoIndexArgs,
) -> Result<ProxyOutcome> {
    run_diff(&materialized.args)
}

pub(super) fn materialize_no_index_fd_args(
    args: &[String],
) -> Result<Option<MaterializedNoIndexArgs>> {
    if !args.iter().any(|arg| arg == "--no-index") {
        return Ok(None);
    }
    let Some(positions) = no_index_fd_positions(args) else {
        return Ok(None);
    };
    let cache_dir = global_cache_root()?.join("git-no-index");
    fs::create_dir_all(&cache_dir).context("failed to create git no-index cache")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut rewritten = args.to_vec();
    let mut files = Vec::new();
    for (ordinal, position) in positions.into_iter().enumerate() {
        let source = &args[position];
        let bytes = fs::read(source)
            .with_context(|| format!("failed to read process-substitution input `{source}`"))?;
        let path = cache_dir.join(format!(
            "{}-{}-{}.tmp",
            stamp,
            std::process::id(),
            ordinal + 1
        ));
        fs::write(&path, bytes).with_context(|| {
            format!(
                "failed to write materialized git input `{}`",
                path.display()
            )
        })?;
        rewritten[position] = path.to_string_lossy().to_string();
        files.push(path);
    }
    Ok(Some(MaterializedNoIndexArgs {
        args: rewritten,
        files,
    }))
}

pub(super) fn no_index_fd_positions(args: &[String]) -> Option<[usize; 2]> {
    let mut positions = Vec::new();
    let mut after_separator = false;
    let mut skip_next = false;
    for (index, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--" {
            after_separator = true;
            continue;
        }
        if !after_separator && git_diff_option_takes_value(arg) {
            skip_next = true;
            continue;
        }
        if !after_separator && arg.starts_with('-') {
            continue;
        }
        if is_fd_path(arg) {
            positions.push(index);
        } else {
            return None;
        }
    }
    match positions.as_slice() {
        [left, right] => Some([*left, *right]),
        _ => None,
    }
}

fn git_diff_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "--output"
            | "--relative"
            | "--diff-filter"
            | "--find-renames"
            | "-M"
            | "--find-copies"
            | "-C"
            | "--inter-hunk-context"
            | "--unified"
            | "-U"
    )
}

fn is_fd_path(arg: &str) -> bool {
    if arg.starts_with("/dev/fd/") || arg.starts_with("/proc/self/fd/") {
        return true;
    }
    let parts = arg.split('/').collect::<Vec<_>>();
    parts.len() >= 5
        && parts[1] == "proc"
        && parts[3] == "fd"
        && parts[2].chars().all(|ch| ch.is_ascii_digit())
        && parts[4].chars().all(|ch| ch.is_ascii_digit())
}

fn is_no_index_difference_exit(
    args: &[String],
    exit_code: i32,
    stdout: &str,
    stderr: &str,
) -> bool {
    exit_code == 1
        && args.iter().any(|arg| arg == "--no-index")
        && !stdout.trim().is_empty()
        && stderr.trim().is_empty()
}

pub(super) fn compact_diff(diff: &str, max_lines: usize) -> String {
    let mut result = Vec::new();
    let mut current_file = String::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut in_hunk = false;
    let mut hunk_shown = 0usize;
    let mut hunk_skipped = 0usize;
    let max_hunk_lines = 100usize;
    let mut was_truncated = false;

    for line in diff.lines() {
        if line.starts_with("diff --git") {
            if hunk_skipped > 0 {
                result.push(format!("  ... ({} lines truncated)", hunk_skipped));
                was_truncated = true;
                hunk_skipped = 0;
            }
            if !current_file.is_empty() && (added > 0 || removed > 0) {
                result.push(format!("  +{} -{}", added, removed));
            }
            current_file = line.split(" b/").nth(1).unwrap_or("unknown").to_string();
            result.push(format!("\n{current_file}"));
            added = 0;
            removed = 0;
            in_hunk = false;
            hunk_shown = 0;
        } else if line.starts_with("@@") {
            if hunk_skipped > 0 {
                result.push(format!("  ... ({} lines truncated)", hunk_skipped));
                was_truncated = true;
                hunk_skipped = 0;
            }
            in_hunk = true;
            hunk_shown = 0;
            let hunk_info = line.split("@@").nth(1).unwrap_or("").trim();
            result.push(format!("  @@ {hunk_info} @@"));
        } else if in_hunk {
            if line.starts_with('+') && !line.starts_with("+++") {
                added += 1;
                if hunk_shown < max_hunk_lines {
                    result.push(format!("  {line}"));
                    hunk_shown += 1;
                } else {
                    hunk_skipped += 1;
                }
            } else if line.starts_with('-') && !line.starts_with("---") {
                removed += 1;
                if hunk_shown < max_hunk_lines {
                    result.push(format!("  {line}"));
                    hunk_shown += 1;
                } else {
                    hunk_skipped += 1;
                }
            } else if hunk_shown < max_hunk_lines && !line.starts_with('\\') && hunk_shown > 0 {
                result.push(format!("  {line}"));
                hunk_shown += 1;
            }
        }

        if result.len() >= max_lines {
            result.push("\n... (more changes truncated)".to_string());
            was_truncated = true;
            break;
        }
    }

    if hunk_skipped > 0 {
        result.push(format!("  ... ({} lines truncated)", hunk_skipped));
        was_truncated = true;
    }
    if !current_file.is_empty() && (added > 0 || removed > 0) {
        result.push(format!("  +{} -{}", added, removed));
    }
    if was_truncated {
        result.push("[full diff: cx git diff --no-compact]".to_string());
    }
    result.join("\n")
}
