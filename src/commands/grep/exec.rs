use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use glob::Pattern;
use walkdir::WalkDir;

use crate::support::runner::{
    capture, capture_with_captured_stdin, capture_with_inherited_stdin, CapturedStdin,
    CommandOutput,
};
use crate::support::utils::resolved_command;

use super::GrepOptions;

pub(super) fn run_rg(
    patterns: &[String],
    paths: &[String],
    options: &GrepOptions,
    captured_stdin: Option<&CapturedStdin>,
) -> Result<CommandOutput> {
    let mut rg = resolved_command("rg");
    rg.args(["-n", "--no-heading", "--color", "never"]);
    if options.ignore_case {
        rg.arg("-i");
    }
    if options.smart_case {
        rg.arg("-S");
    }
    if let Some(lines) = options.context_lines {
        rg.arg("-C").arg(lines.to_string());
    }
    if let Some(lines) = options.context_before {
        rg.arg("-B").arg(lines.to_string());
    }
    if let Some(lines) = options.context_after {
        rg.arg("-A").arg(lines.to_string());
    }
    if options.files_with_matches {
        rg.arg("-l");
    }
    if options.hidden {
        rg.arg("--hidden");
    }
    if options.no_ignore {
        rg.arg("--no-ignore");
    }
    if options.text {
        rg.arg("-a");
    }
    if options.only_matching {
        rg.arg("-o");
    }
    if options.fixed_strings {
        rg.arg("-F");
    }
    for glob in &options.glob_patterns {
        rg.arg("-g").arg(glob);
    }
    for pattern in patterns {
        rg.arg("-e").arg(pattern);
    }
    rg.args(paths);
    capture_search(rg, "rg", paths, captured_stdin)
}

pub(super) fn should_retry_with_grep_fallback(
    output: &CommandOutput,
    options: &GrepOptions,
) -> bool {
    !options.fixed_strings && output.exit_code == 2 && output.stderr.contains("regex parse error")
}

pub(super) fn run_rg_files(paths: &[String], options: &GrepOptions) -> Result<CommandOutput> {
    let mut rg = resolved_command("rg");
    rg.arg("--files");
    if options.hidden {
        rg.arg("--hidden");
    }
    if options.no_ignore {
        rg.arg("--no-ignore");
    }
    for glob in &options.glob_patterns {
        rg.arg("-g").arg(glob);
    }
    rg.args(paths);
    capture(rg, "rg").context("failed to run rg --files")
}

pub(super) fn run_grep_fallback(
    patterns: &[String],
    paths: &[String],
    options: &GrepOptions,
    captured_stdin: Option<&CapturedStdin>,
) -> Result<CommandOutput> {
    let search_inputs = expand_fallback_paths(paths, &options.glob_patterns)?;
    if search_inputs.is_empty() {
        return Ok(empty_match_output(1));
    }

    let mut grep = resolved_command("grep");
    grep.arg("-rn");
    if options.ignore_case {
        grep.arg("-i");
    }
    if let Some(lines) = options.context_lines {
        grep.arg("-C").arg(lines.to_string());
    }
    if let Some(lines) = options.context_before {
        grep.arg("-B").arg(lines.to_string());
    }
    if let Some(lines) = options.context_after {
        grep.arg("-A").arg(lines.to_string());
    }
    if options.files_with_matches {
        grep.arg("-l");
    }
    if options.text {
        grep.arg("-a");
    }
    if options.only_matching {
        grep.arg("-o");
    }
    if options.fixed_strings {
        grep.arg("-F");
    } else if options.extended_regexp {
        grep.arg("-E");
    }
    for pattern in patterns {
        grep.arg("-e").arg(pattern);
    }
    grep.args(&search_inputs);
    capture_search(grep, "grep", &search_inputs, captured_stdin)
}

fn capture_search(
    command: std::process::Command,
    tool_name: &str,
    paths: &[String],
    captured_stdin: Option<&CapturedStdin>,
) -> Result<CommandOutput> {
    if let Some(stdin) = captured_stdin {
        capture_with_captured_stdin(command, tool_name, stdin)
    } else if paths.iter().any(|path| path == "-") {
        capture_with_inherited_stdin(command, tool_name)
    } else {
        capture(command, tool_name)
    }
}

fn empty_match_output(code: i32) -> CommandOutput {
    CommandOutput::from_combined(String::new(), code)
}

pub(super) fn collect_files_fallback(
    paths: &[String],
    options: &GrepOptions,
) -> Result<Vec<String>> {
    let compiled_patterns = compile_glob_patterns(&options.glob_patterns)?;
    let mut files = BTreeSet::new();

    for raw_path in paths {
        let path = PathBuf::from(raw_path);
        if !path.exists() {
            continue;
        }
        if path.is_file() {
            if should_include_path(
                &path,
                path.parent().unwrap_or(Path::new(".")),
                &compiled_patterns,
                options.hidden,
            ) {
                files.insert(path.display().to_string());
            }
            continue;
        }

        for entry in WalkDir::new(&path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }
            if should_include_path(entry_path, &path, &compiled_patterns, options.hidden) {
                files.insert(entry_path.display().to_string());
            }
        }
    }

    Ok(files.into_iter().collect())
}

fn expand_fallback_paths(paths: &[String], glob_patterns: &[String]) -> Result<Vec<String>> {
    if glob_patterns.is_empty() {
        return Ok(paths.to_vec());
    }

    let compiled_patterns = compile_glob_patterns(glob_patterns)?;

    let mut expanded = BTreeSet::new();
    for raw_path in paths {
        let path = PathBuf::from(raw_path);
        if !path.exists() {
            expanded.insert(raw_path.clone());
            continue;
        }
        if path.is_file() {
            if path_matches_globs(
                &path,
                path.parent().unwrap_or(Path::new(".")),
                &compiled_patterns,
            ) {
                expanded.insert(path.display().to_string());
            }
            continue;
        }
        for entry in WalkDir::new(&path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }
            if path_matches_globs(entry_path, &path, &compiled_patterns) {
                expanded.insert(entry_path.display().to_string());
            }
        }
    }
    Ok(expanded.into_iter().collect())
}

fn compile_glob_patterns(glob_patterns: &[String]) -> Result<Vec<(String, Pattern)>> {
    glob_patterns
        .iter()
        .map(|pattern| {
            Ok((
                pattern.clone(),
                Pattern::new(pattern)
                    .with_context(|| format!("invalid glob pattern `{pattern}`"))?,
            ))
        })
        .collect::<Result<Vec<(String, Pattern)>>>()
}

fn should_include_path(
    path: &Path,
    root: &Path,
    patterns: &[(String, Pattern)],
    include_hidden: bool,
) -> bool {
    (include_hidden || !path_is_hidden(path, root))
        && (patterns.is_empty() || path_matches_globs(path, root, patterns))
}

fn path_is_hidden(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|part| part.starts_with('.') && part.len() > 1)
    })
}

fn path_matches_globs(path: &Path, root: &Path, patterns: &[(String, Pattern)]) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let relative_string = relative.to_string_lossy().replace('\\', "/");
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();

    patterns.iter().any(|(source, pattern)| {
        pattern.matches(&relative_string)
            || pattern.matches_path(relative)
            || (!source.contains('/') && pattern.matches(file_name))
    })
}
