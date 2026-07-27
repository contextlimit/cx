use anyhow::Result;

use crate::support::runner::{run_filtered, ProxyOutcome, RunOptions};
use crate::support::utils::{resolved_command, tool_exists, truncate};

#[derive(Debug, PartialEq)]
enum ParseState {
    Header,
    TestProgress,
    Failures,
    Summary,
}

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = if tool_exists("pytest") {
        resolved_command("pytest")
    } else {
        let mut fallback = resolved_command("python");
        fallback.arg("-m").arg("pytest");
        fallback
    };

    if !args.iter().any(|arg| arg.starts_with("--tb")) {
        cmd.arg("--tb=short");
    }
    if !args.iter().any(|arg| arg == "-q" || arg == "--quiet") {
        cmd.arg("-q");
    }
    for arg in args {
        cmd.arg(arg);
    }

    run_filtered(
        cmd,
        "pytest",
        |output| Some(filter_pytest_output(&output.stdout)),
        RunOptions::stdout_only(),
    )
}

fn filter_pytest_output(output: &str) -> String {
    let mut state = ParseState::Header;
    let mut test_files = Vec::new();
    let mut failures = Vec::new();
    let mut current_failure = Vec::new();
    let mut summary_line = String::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("===") && trimmed.contains("test session starts") {
            state = ParseState::Header;
            continue;
        } else if trimmed.starts_with("===") && trimmed.contains("FAILURES") {
            state = ParseState::Failures;
            continue;
        } else if trimmed.starts_with("===") && trimmed.contains("short test summary") {
            state = ParseState::Summary;
            if !current_failure.is_empty() {
                failures.push(current_failure.join("\n"));
                current_failure.clear();
            }
            continue;
        } else if trimmed.starts_with("===")
            && (trimmed.contains("passed") || trimmed.contains("failed"))
        {
            summary_line = trimmed.to_string();
            continue;
        }

        match state {
            ParseState::Header => {
                if trimmed.starts_with("collected") {
                    state = ParseState::TestProgress;
                }
            }
            ParseState::TestProgress => {
                if !trimmed.is_empty()
                    && !trimmed.starts_with("===")
                    && (trimmed.contains(".py") || trimmed.contains("%]"))
                {
                    test_files.push(trimmed.to_string());
                }
            }
            ParseState::Failures => {
                if trimmed.starts_with("___") {
                    if !current_failure.is_empty() {
                        failures.push(current_failure.join("\n"));
                        current_failure.clear();
                    }
                    current_failure.push(trimmed.to_string());
                } else if !trimmed.is_empty() && !trimmed.starts_with("===") {
                    current_failure.push(trimmed.to_string());
                }
            }
            ParseState::Summary => {
                if trimmed.starts_with("FAILED") || trimmed.starts_with("ERROR") {
                    failures.push(trimmed.to_string());
                }
            }
        }
    }

    if !current_failure.is_empty() {
        failures.push(current_failure.join("\n"));
    }

    build_pytest_summary(&summary_line, &test_files, &failures)
}

fn build_pytest_summary(summary: &str, _test_files: &[String], failures: &[String]) -> String {
    let (passed, failed, skipped) = parse_summary_line(summary);
    if failed == 0 && passed > 0 {
        return format!("Pytest: {passed} passed");
    }
    if passed == 0 && failed == 0 {
        return "Pytest: No tests collected".to_string();
    }

    let mut result = format!("Pytest: {passed} passed, {failed} failed");
    if skipped > 0 {
        result.push_str(&format!(", {skipped} skipped"));
    }
    result.push('\n');
    result.push_str("═══════════════════════════════════════\n");

    if failures.is_empty() {
        return result.trim().to_string();
    }

    result.push_str("\nFailures:\n");
    for (index, failure) in failures.iter().take(5).enumerate() {
        let lines: Vec<&str> = failure.lines().collect();
        if let Some(first_line) = lines.first() {
            if first_line.starts_with("___") {
                result.push_str(&format!(
                    "{}. [FAIL] {}\n",
                    index + 1,
                    first_line.trim_matches('_').trim()
                ));
            } else if first_line.starts_with("FAILED") {
                let parts: Vec<&str> = first_line.split(" - ").collect();
                let test_name = parts
                    .first()
                    .copied()
                    .unwrap_or("")
                    .trim_start_matches("FAILED ");
                result.push_str(&format!("{}. [FAIL] {}\n", index + 1, test_name));
                if parts.len() > 1 {
                    result.push_str(&format!("     {}\n", truncate(parts[1], 100)));
                }
                continue;
            }
        }

        let mut relevant_lines = 0usize;
        for line in lines.iter().skip(1) {
            let lower = line.to_ascii_lowercase();
            let relevant = line.trim().starts_with('>')
                || line.trim().starts_with('E')
                || lower.contains("assert")
                || lower.contains("error")
                || line.contains(".py:");
            if relevant && relevant_lines < 3 {
                result.push_str(&format!("     {}\n", truncate(line, 100)));
                relevant_lines += 1;
            }
        }
        if index + 1 < failures.len() {
            result.push('\n');
        }
    }

    if failures.len() > 5 {
        result.push_str(&format!("\n... +{} more failures\n", failures.len() - 5));
    }

    result.trim().to_string()
}

fn parse_summary_line(summary: &str) -> (usize, usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    for part in summary.split(',') {
        let words: Vec<&str> = part.split_whitespace().collect();
        for (index, word) in words.iter().enumerate() {
            if index == 0 {
                continue;
            }
            if word.contains("passed") {
                passed = words[index - 1].parse().unwrap_or(0);
            } else if word.contains("failed") {
                failed = words[index - 1].parse().unwrap_or(0);
            } else if word.contains("skipped") {
                skipped = words[index - 1].parse().unwrap_or(0);
            }
        }
    }
    (passed, failed, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_pytest_all_pass() {
        let output = "=== test session starts ===\ncollected 5 items\n\ntests/test_foo.py ..... [100%]\n\n=== 5 passed in 0.50s ===";
        assert_eq!(filter_pytest_output(output), "Pytest: 5 passed");
    }

    #[test]
    fn filter_pytest_failures_show_context() {
        let output = "=== test session starts ===\ncollected 1 item\n\n=== FAILURES ===\n___ test_alpha ___\n> assert 1 == 2\nE AssertionError\n=== short test summary info ===\nFAILED tests/test_alpha.py::test_alpha - AssertionError\n=== 0 passed, 1 failed in 0.10s ===";
        let filtered = filter_pytest_output(output);
        assert!(filtered.contains("1 failed"));
        assert!(filtered.contains("test_alpha"));
        assert!(filtered.contains("AssertionError"));
    }

    #[cfg(unix)]
    #[test]
    fn run_uses_fake_pytest_binary() {
        crate::support::test_support::with_fake_path(
            &[(
                "pytest",
                "#!/bin/sh\ncat <<'EOF'\n=== test session starts ===\ncollected 1 item\n\ntests/test_ok.py . [100%]\n\n=== 1 passed in 0.10s ===\nEOF\n",
            )],
            || {
                let output = run(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert_eq!(output.stdout, "Pytest: 1 passed");
            },
        );
    }
}
