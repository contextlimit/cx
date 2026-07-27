use anyhow::Result;

use crate::support::runner::{append_failure_hint, capture, ProxyOutcome};
use crate::support::utils::resolved_command;

pub fn run_status(args: &[String]) -> Result<ProxyOutcome> {
    if !args.is_empty() {
        let mut cmd = resolved_command("git");
        cmd.arg("status").args(args);
        let mut output = capture(cmd, "git status")?;
        let exit_code = output.exit_code;
        if exit_code != 0 {
            let failure_hint = output.failure_artifact_hint("git");
            let observation = output.observation("git status");
            return Ok(ProxyOutcome {
                stdout: append_failure_hint(output.stdout, failure_hint.as_deref()),
                stderr: output.stderr.trim_end().to_string(),
                exit_code,
                observation: None,
            }
            .with_observation(observation));
        }
        if requires_exact_status_output(args) {
            let observation = output
                .observation("git status")
                .with_preserved_stream_termination();
            return Ok(ProxyOutcome {
                stdout: std::mem::take(&mut output.stdout),
                stderr: std::mem::take(&mut output.stderr),
                exit_code,
                observation: None,
            }
            .with_observation(observation));
        }
        return Ok(ProxyOutcome {
            stdout: filter_status_with_args(&output.stdout),
            stderr: output.stderr.trim_end().to_string(),
            exit_code,
            observation: None,
        }
        .with_observation(output.observation("git status"))
        .with_expansion_reason("status-summary"));
    }

    let mut cmd = resolved_command("git");
    cmd.args(["status", "--porcelain", "-b"]);
    let mut output = capture(cmd, "git status")?;
    let exit_code = output.exit_code;
    if exit_code != 0 {
        let failure_hint = output.failure_artifact_hint("git");
        let observation = output.observation("git status");
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(output.stdout, failure_hint.as_deref()),
            stderr: output.stderr.trim_end().to_string(),
            exit_code,
            observation: None,
        }
        .with_observation(observation));
    }

    Ok(ProxyOutcome {
        stdout: format_status_output(&output.stdout),
        stderr: String::new(),
        exit_code,
        observation: None,
    }
    .with_observation(output.observation("git status"))
    .with_expansion_reason("status-summary"))
}

fn requires_exact_status_output(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--short" | "-s" | "--porcelain" | "-z" | "--null"
        ) || arg.starts_with("--porcelain=")
    })
}

pub(super) fn format_status_output(porcelain: &str) -> String {
    let lines: Vec<&str> = porcelain.lines().collect();
    if lines.is_empty() {
        return "Clean working tree".to_string();
    }
    let mut output = String::new();
    if let Some(branch_line) = lines.first() {
        if branch_line.starts_with("##") {
            output.push_str(&format!("* {}\n", branch_line.trim_start_matches("## ")));
        }
    }

    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();
    let mut conflicts = 0usize;

    for line in lines.iter().skip(1) {
        if line.len() < 3 {
            continue;
        }
        let status = line.get(0..2).unwrap_or("  ");
        let file = line.get(3..).unwrap_or("");
        match status.chars().next().unwrap_or(' ') {
            'M' | 'A' | 'D' | 'R' | 'C' => staged.push(file),
            'U' => conflicts += 1,
            _ => {}
        }
        match status.chars().nth(1).unwrap_or(' ') {
            'M' | 'D' => modified.push(file),
            _ => {}
        }
        if status == "??" {
            untracked.push(file);
        }
    }

    if !staged.is_empty() {
        output.push_str(&format!("+ Staged: {} files\n", staged.len()));
        for file in staged.iter().take(15) {
            output.push_str(&format!("   {file}\n"));
        }
    }
    if !modified.is_empty() {
        output.push_str(&format!("~ Modified: {} files\n", modified.len()));
        for file in modified.iter().take(15) {
            output.push_str(&format!("   {file}\n"));
        }
    }
    if !untracked.is_empty() {
        output.push_str(&format!("? Untracked: {} files\n", untracked.len()));
        for file in untracked.iter().take(10) {
            output.push_str(&format!("   {file}\n"));
        }
    }
    if conflicts > 0 {
        output.push_str(&format!("conflicts: {} files\n", conflicts));
    }
    if staged.is_empty() && modified.is_empty() && untracked.is_empty() && conflicts == 0 {
        output.push_str("clean - nothing to commit\n");
    }
    output.trim_end().to_string()
}

fn filter_status_with_args(output: &str) -> String {
    let mut lines = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("(use \"git")
            || trimmed.starts_with("(create/copy files")
            || trimmed.contains("(use \"git add")
            || trimmed.contains("(use \"git restore")
        {
            continue;
        }
        if trimmed.contains("nothing to commit") && trimmed.contains("working tree clean") {
            lines.push(trimmed.to_string());
            break;
        }
        lines.push(line.to_string());
    }
    if lines.is_empty() {
        "ok".to_string()
    } else {
        lines.join("\n")
    }
}
