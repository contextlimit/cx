use anyhow::Result;

use crate::support::runner::{append_failure_hint, capture, ProxyOutcome};
use crate::support::utils::{resolved_command, truncate};

pub fn run_log(args: &[String]) -> Result<ProxyOutcome> {
    if args.iter().any(|arg| arg == "--no-compact") {
        return run_direct_log(args);
    }
    let mut cmd = resolved_command("git");
    cmd.arg("log");
    let has_format_flag = has_user_format(args);
    let has_limit_flag = args.iter().any(|arg| {
        (arg.starts_with('-') && arg.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit()))
            || arg == "-n"
            || arg.starts_with("--max-count")
    });

    if !has_format_flag {
        cmd.args(["--pretty=format:%h %s (%ar) <%an>%n%b%n---END---"]);
    }
    let (limit, user_set_limit) = if has_limit_flag {
        (parse_user_limit(args).unwrap_or(10), true)
    } else if has_format_flag {
        cmd.arg("-50");
        (50, false)
    } else {
        cmd.arg("-10");
        (10, false)
    };
    if !has_format_flag && !has_explicit_parent_policy(args) {
        cmd.arg("--no-merges");
    }
    cmd.args(args);

    let mut output = capture(cmd, "git log")?;
    let exit_code = output.exit_code;
    if exit_code != 0 {
        let failure_hint = output.failure_artifact_hint("git");
        let observation = output.observation("git log");
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(output.stdout, failure_hint.as_deref()),
            stderr: output.stderr.trim_end().to_string(),
            exit_code,
            observation: None,
        }
        .with_observation(observation));
    }
    if has_format_flag {
        let observation = output
            .observation("git log")
            .with_preserved_stream_termination();
        return Ok(ProxyOutcome {
            stdout: std::mem::take(&mut output.stdout),
            stderr: std::mem::take(&mut output.stderr),
            exit_code,
            observation: None,
        }
        .with_observation(observation));
    }

    Ok(ProxyOutcome {
        stdout: filter_log_output(&output.stdout, limit, user_set_limit),
        stderr: String::new(),
        exit_code,
        observation: None,
    }
    .with_observation(output.observation("git log")))
}

fn run_direct_log(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("git");
    cmd.arg("log");
    for arg in args {
        if arg != "--no-compact" {
            cmd.arg(arg);
        }
    }
    let mut output = capture(cmd, "git log")?;
    let exit_code = output.exit_code;
    if exit_code != 0 {
        let failure_hint = output.failure_artifact_hint("git");
        let observation = output.observation("git log");
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(output.stdout, failure_hint.as_deref()),
            stderr: output.stderr.trim_end().to_string(),
            exit_code,
            observation: None,
        }
        .with_observation(observation));
    }
    let observation = output
        .observation("git log")
        .with_preserved_stream_termination();
    Ok(ProxyOutcome {
        stdout: std::mem::take(&mut output.stdout),
        stderr: std::mem::take(&mut output.stderr),
        exit_code,
        observation: None,
    }
    .with_observation(observation))
}

pub fn run_show(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("git");
    cmd.arg("show");
    for arg in args {
        if arg != "--no-compact" {
            cmd.arg(arg);
        }
    }
    let mut output = capture(cmd, "git show")?;
    let exit_code = output.exit_code;
    let failure_hint = if exit_code == 0 {
        None
    } else {
        output.failure_artifact_hint("git")
    };
    let observation = output.observation("git show");
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

fn parse_user_limit(args: &[String]) -> Option<usize> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg.starts_with('-')
            && arg.len() > 1
            && arg.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit())
        {
            if let Ok(limit) = arg[1..].parse::<usize>() {
                return Some(limit);
            }
        }
        if arg == "-n" {
            if let Some(next) = iter.next() {
                if let Ok(limit) = next.parse::<usize>() {
                    return Some(limit);
                }
            }
        }
        if let Some(rest) = arg.strip_prefix("--max-count=") {
            if let Ok(limit) = rest.parse::<usize>() {
                return Some(limit);
            }
        }
        if arg == "--max-count" {
            if let Some(next) = iter.next() {
                if let Ok(limit) = next.parse::<usize>() {
                    return Some(limit);
                }
            }
        }
    }
    None
}

fn has_user_format(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--oneline" || arg.starts_with("--pretty") || arg.starts_with("--format"))
}

fn has_explicit_parent_policy(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--first-parent"
                | "--merges"
                | "--no-merges"
                | "--min-parents"
                | "--max-parents"
                | "--no-min-parents"
                | "--no-max-parents"
        ) || arg.starts_with("--min-parents=")
            || arg.starts_with("--max-parents=")
    })
}

pub(super) fn filter_log_output(output: &str, limit: usize, user_set_limit: bool) -> String {
    let truncate_width = if user_set_limit { 120 } else { 80 };
    let commits: Vec<&str> = output.split("---END---").collect();
    let max_commits = if user_set_limit { commits.len() } else { limit };
    let mut result = Vec::new();
    for block in commits.iter().take(max_commits) {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        let mut lines = block.lines();
        let Some(header) = lines.next() else {
            continue;
        };
        let mut entry = truncate_line(header.trim(), truncate_width);
        let body_lines: Vec<&str> = lines
            .map(str::trim)
            .filter(|line| {
                !line.is_empty()
                    && !line.starts_with("Signed-off-by:")
                    && !line.starts_with("Co-authored-by:")
            })
            .collect();
        let omitted = body_lines.len().saturating_sub(3);
        for body in body_lines.iter().take(3) {
            entry.push_str(&format!("\n  {}", truncate_line(body, truncate_width)));
        }
        if omitted > 0 {
            entry.push_str(&format!("\n  [+{} lines omitted]", omitted));
        }
        result.push(entry);
    }
    result.join("\n").trim().to_string()
}

fn truncate_line(line: &str, width: usize) -> String {
    truncate(line, width)
}
