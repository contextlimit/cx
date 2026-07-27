use anyhow::Result;
use std::collections::{HashSet, VecDeque};

use crate::support::runner::{
    append_failure_hint, capture, run_filtered, CommandOutput, ProxyOutcome, RunOptions,
};
use crate::support::utils::{fallback_window, resolved_command, truncate};

const CTEST_LIST_HEAD_LINES: usize = 80;
const CTEST_LIST_TAIL_LINES: usize = 40;
const CTEST_LIST_LINE_PREVIEW_CHARS: usize = 320;
const CTEST_SMALL_EXACT_LINES: usize = 20;
const CTEST_SMALL_EXACT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CtestMode {
    Run,
    ListText,
    ListMachine,
}

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    let mode = ctest_mode(args);
    let mut cmd = resolved_command("ctest");
    for arg in args {
        cmd.arg(arg);
    }

    match mode {
        CtestMode::Run => {
            Ok(
                run_filtered(cmd, "ctest", filter_ctest_output, RunOptions::default())?
                    .with_expansion_reason("test-result-summary"),
            )
        }
        CtestMode::ListText | CtestMode::ListMachine => run_ctest_list(cmd, mode),
    }
}

fn ctest_mode(args: &[String]) -> CtestMode {
    let mut list_mode = false;
    for (index, arg) in args.iter().enumerate() {
        if arg == "-N" {
            list_mode = true;
        } else if let Some(format) = arg.strip_prefix("--show-only=") {
            return if format.starts_with("json") {
                CtestMode::ListMachine
            } else {
                CtestMode::ListText
            };
        } else if arg == "--show-only" {
            list_mode = true;
            if args
                .get(index + 1)
                .is_some_and(|format| format.starts_with("json"))
            {
                return CtestMode::ListMachine;
            }
        }
    }
    if list_mode {
        CtestMode::ListText
    } else {
        CtestMode::Run
    }
}

fn run_ctest_list(cmd: std::process::Command, mode: CtestMode) -> Result<ProxyOutcome> {
    let mut output = capture(cmd, "ctest")?;
    let exact = mode == CtestMode::ListMachine || should_preserve_small_ctest_list(&output);
    let exit_code = output.exit_code;
    let failure_hint = if exit_code == 0 {
        None
    } else {
        output.failure_artifact_hint("ctest")
    };
    let mut observation = output.observation("ctest");
    if exact && exit_code == 0 {
        observation = observation.with_preserved_stream_termination();
        return Ok(ProxyOutcome {
            stdout: std::mem::take(&mut output.stdout),
            stderr: std::mem::take(&mut output.stderr),
            exit_code,
            observation: None,
        }
        .with_observation(observation));
    }

    let stdout = filter_ctest_list_output(&output).unwrap_or_default();
    Ok(ProxyOutcome {
        stdout: append_failure_hint(stdout, failure_hint.as_deref()),
        stderr: String::new(),
        exit_code,
        observation: None,
    }
    .with_observation(observation)
    .with_expansion_reason("test-catalog-formatting"))
}

fn should_preserve_small_ctest_list(output: &CommandOutput) -> bool {
    output.exit_code == 0
        && output.stderr.is_empty()
        && !output.stdout.is_empty()
        && output.stdout.len() <= CTEST_SMALL_EXACT_BYTES
        && output.stdout.lines().count() <= CTEST_SMALL_EXACT_LINES
}

fn filter_ctest_list_output(output: &CommandOutput) -> Option<String> {
    let headline = if output.exit_code == 0 {
        "ctest: list"
    } else {
        "ctest: list failed"
    };
    let mut lines = CtestListWindow::new(CTEST_LIST_HEAD_LINES, CTEST_LIST_TAIL_LINES);
    for line in output
        .combined
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        lines.push(line);
    }

    if lines.is_empty() {
        return Some(headline.to_string());
    }

    let mut result = headline.to_string();
    result.push('\n');
    result.push_str("═══════════════════════════════════════\n");
    lines.render_into(&mut result, CTEST_LIST_LINE_PREVIEW_CHARS);
    Some(result.trim_end().to_string())
}

struct CtestListWindow<'a> {
    head_lines: usize,
    tail_lines: usize,
    total_lines: usize,
    all_lines: Vec<&'a str>,
    head: Vec<&'a str>,
    tail: VecDeque<&'a str>,
}

impl<'a> CtestListWindow<'a> {
    fn new(head_lines: usize, tail_lines: usize) -> Self {
        Self {
            head_lines,
            tail_lines,
            total_lines: 0,
            all_lines: Vec::with_capacity(head_lines.saturating_add(tail_lines)),
            head: Vec::with_capacity(head_lines),
            tail: VecDeque::with_capacity(tail_lines),
        }
    }

    fn is_empty(&self) -> bool {
        self.total_lines == 0
    }

    fn push(&mut self, line: &'a str) {
        self.total_lines += 1;
        if self.head.len() < self.head_lines {
            self.head.push(line);
        }
        if self.tail_lines > 0 {
            if self.tail.len() == self.tail_lines {
                self.tail.pop_front();
            }
            self.tail.push_back(line);
        }
        if self.total_lines <= self.window_size() {
            self.all_lines.push(line);
        }
    }

    fn render_into(&self, result: &mut String, max_line_chars: usize) {
        if self.total_lines <= self.window_size() {
            append_truncated_lines(result, self.all_lines.iter().copied(), max_line_chars);
            return;
        }

        append_truncated_lines(result, self.head.iter().copied(), max_line_chars);
        result.push_str(&format!(
            "... [{} lines omitted] ...\n",
            self.total_lines - self.window_size()
        ));
        append_truncated_lines(result, self.tail.iter().copied(), max_line_chars);
    }

    fn window_size(&self) -> usize {
        self.head_lines.saturating_add(self.tail_lines)
    }
}

fn append_truncated_lines<'a>(
    result: &mut String,
    lines: impl IntoIterator<Item = &'a str>,
    max_line_chars: usize,
) {
    for line in lines {
        result.push_str(&truncate(line, max_line_chars));
        result.push('\n');
    }
}

fn filter_ctest_output(output: &CommandOutput) -> Option<String> {
    let lines: Vec<&str> = output
        .combined
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return Some(if output.exit_code == 0 {
            "ctest: ok".to_string()
        } else {
            "ctest: failed".to_string()
        });
    }

    let summary: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| is_ctest_summary(line))
        .collect();
    let failures: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| is_ctest_failure(line))
        .collect();

    if output.exit_code != 0 && failures.is_empty() && summary.is_empty() {
        return None;
    }

    let mut result = if output.exit_code == 0 {
        "ctest: ok".to_string()
    } else {
        failure_headline(&summary, failures.len())
    };

    let selected = collect_selected_lines(&lines);
    if selected.is_empty() {
        result.push('\n');
        result.push_str(&fallback_window(&lines.join("\n"), 8, 18));
        return Some(result);
    }

    result.push('\n');
    result.push_str("═══════════════════════════════════════\n");
    for line in selected.iter().take(80) {
        result.push_str(&truncate(line.trim(), 240));
        result.push('\n');
    }
    if selected.len() > 80 {
        result.push_str(&format!("... +{} more lines\n", selected.len() - 80));
    }
    Some(result.trim_end().to_string())
}

fn collect_selected_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    let mut failure_context_remaining = 0usize;

    for line in lines {
        let summary = is_ctest_summary(line);
        let failure = is_ctest_failure(line);
        let diagnostic = is_ctest_diagnostic(line);
        let failure_context = failure_context_remaining > 0
            && !summary
            && !looks_like_json_line(line)
            && !line.trim().is_empty();

        if !(summary || failure || diagnostic || failure_context) {
            if failure_context_remaining > 0 && looks_like_json_line(line) {
                failure_context_remaining = failure_context_remaining.saturating_sub(1);
            }
            continue;
        }

        let key = line.trim().to_string();
        if seen.insert(key) {
            selected.push(*line);
        }

        if line.contains("***Failed") {
            failure_context_remaining = 6;
        } else if failure_context_remaining > 0 && failure_context {
            failure_context_remaining = failure_context_remaining.saturating_sub(1);
        } else if failure_context_remaining > 0 && summary {
            failure_context_remaining = 0;
        }
    }

    selected
}

fn failure_headline(summary: &[&str], failure_lines: usize) -> String {
    let failed_tests = summary
        .iter()
        .find_map(|line| parse_failed_test_count(line));
    match failed_tests {
        Some(failed_tests) => format!("ctest: failed ({failed_tests} tests failed)"),
        None => format!("ctest: failed ({failure_lines} failure lines)"),
    }
}

fn parse_failed_test_count(line: &str) -> Option<usize> {
    let lower = line.to_ascii_lowercase();
    let marker = "tests failed out of";
    let marker_index = lower.find(marker)?;
    let prefix = lower[..marker_index].trim_end();
    prefix
        .split_whitespace()
        .last()
        .and_then(|value| value.parse::<usize>().ok())
}

fn is_ctest_summary(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("test project")
        || lower.contains("tests passed")
        || lower.contains("total test time")
        || lower.starts_with("start ")
        || looks_like_ctest_result_line(trimmed, &lower)
}

fn is_ctest_failure(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("***failed")
        || lower.contains(" failed")
        || lower.contains("errors while running ctest")
        || lower.contains("the following tests failed")
        || lower.contains("subprocess aborted")
        || lower.contains("segfault")
        || lower.contains("exception")
}

fn looks_like_ctest_result_line(trimmed: &str, lower: &str) -> bool {
    trimmed.contains('#')
        && lower.contains("test")
        && (lower.contains("passed")
            || lower.contains("***failed")
            || lower.contains("***not run")
            || lower.contains("***exception"))
}

fn is_ctest_diagnostic(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("error:")
        || lower.contains("assertionerror")
        || lower.contains("target crashed")
        || lower.contains("browser page crashed")
        || lower.contains("triggeruncaughtexception")
        || lower.contains("page.evaluate:")
        || lower.contains("timed out waiting")
        || lower.starts_with("last good state:")
        || lower.starts_with("node.js v")
        || (trimmed.starts_with('[')
            && (lower.contains("error")
                || lower.contains("failed")
                || lower.contains("browser")
                || lower.contains("cleaned up")))
}

fn looks_like_json_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('{')
        || trimmed.starts_with('}')
        || trimmed.starts_with('"')
        || trimmed.starts_with(']')
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn filter_ctest_output_summarizes_success() {
        let output = CommandOutput::from_combined(
            "Test project build\n    Start 1: dock-tests\n1/1 Test #1: dock-tests ....   Passed 0.12 sec\n100% tests passed, 0 tests failed out of 1\nTotal Test time (real) = 0.12 sec\n",
            0,
        );
        let filtered = filter_ctest_output(&output).unwrap();
        assert!(filtered.contains("ctest: ok"));
        assert!(filtered.contains("100% tests passed"));
    }

    #[test]
    fn detects_ctest_list_modes() {
        assert_eq!(ctest_mode(&["-N".to_string()]), CtestMode::ListText);
        assert_eq!(
            ctest_mode(&["--show-only".to_string()]),
            CtestMode::ListText
        );
        assert_eq!(
            ctest_mode(&["--show-only=json-v1".to_string()]),
            CtestMode::ListMachine
        );
        assert_eq!(
            ctest_mode(&["--show-only".to_string(), "json-v1".to_string()]),
            CtestMode::ListMachine
        );
        assert_eq!(
            ctest_mode(&["--output-on-failure".to_string()]),
            CtestMode::Run
        );
    }

    #[test]
    fn filter_ctest_list_output_preserves_catalog() {
        let output = CommandOutput::from_combined(
            "Test project /work/build\n  Test #71: sample-ui-chat-browser-collaboration-model-tests\n  Test #72: sample-ui-chat-markdown-model-tests\nTotal Tests: 2\n",
            0,
        );
        let filtered = filter_ctest_list_output(&output).unwrap();
        assert!(filtered.contains("ctest: list"));
        assert!(!filtered.contains("ctest: ok"));
        assert!(filtered.contains("sample-ui-chat-browser-collaboration-model-tests"));
        assert!(filtered.contains("sample-ui-chat-markdown-model-tests"));
        assert!(filtered.contains("Total Tests: 2"));
    }

    #[test]
    fn filter_ctest_list_output_windows_large_catalog() {
        let mut combined = "Test project /work/build\n".to_string();
        for index in 0..150 {
            combined.push_str(&format!("  Test #{index}: sample-ui-catalog-{index:03}\n"));
        }
        combined.push_str("Total Tests: 150\n");
        let output = CommandOutput::from_combined(combined, 0);
        let filtered = filter_ctest_list_output(&output).unwrap();
        assert!(filtered.contains("ctest: list"));
        assert!(filtered.contains("sample-ui-catalog-000"));
        assert!(filtered.contains("sample-ui-catalog-149"));
        assert!(filtered.contains("Total Tests: 150"));
        assert!(filtered.contains("lines omitted"));
        assert!(!filtered.contains("sample-ui-catalog-090"));
    }

    #[test]
    fn filter_ctest_output_summarizes_failures() {
        let output = CommandOutput::from_combined(
            "Test project build\n1/2 Test #1: dock-tests ....***Failed 0.12 sec\n50% tests passed, 1 tests failed out of 2\nThe following tests FAILED:\n\t1 - dock-tests (Failed)\nErrors while running CTest\n",
            8,
        );
        let filtered = filter_ctest_output(&output).unwrap();
        assert!(filtered.contains("ctest: failed (1 tests failed)"));
        assert!(filtered.contains("dock-tests"));
        assert!(filtered.contains("Errors while running CTest"));
    }

    #[test]
    fn filter_ctest_output_preserves_original_line_order() {
        let output = CommandOutput::from_combined(
            "Test project build\n    Start  2: snapshot\n1/3 Test  #2: snapshot .........   Passed    0.10 sec\n    Start  7: smoke\n2/3 Test  #7: smoke ............***Failed   0.20 sec\ntriggerUncaughtException(\n66% tests passed, 1 tests failed out of 3\nErrors while running CTest\n",
            8,
        );
        let filtered = filter_ctest_output(&output).unwrap();
        let start_snapshot = filtered
            .find("Start  2: snapshot")
            .unwrap_or_else(|| filtered.find("Start 2: snapshot").unwrap());
        let pass_snapshot = filtered
            .find("Test  #2: snapshot")
            .unwrap_or_else(|| filtered.find("Test #2: snapshot").unwrap());
        let start_smoke = filtered
            .find("Start  7: smoke")
            .unwrap_or_else(|| filtered.find("Start 7: smoke").unwrap());
        let fail_smoke = filtered
            .find("Test  #7: smoke")
            .unwrap_or_else(|| filtered.find("Test #7: smoke").unwrap());
        assert!(start_snapshot < pass_snapshot);
        assert!(pass_snapshot < start_smoke);
        assert!(start_smoke < fail_smoke);
    }

    #[test]
    fn filter_ctest_output_keeps_failure_diagnostics_without_json_blob() {
        let output = CommandOutput::from_combined(
            "Test project /tmp/build\n    Start 53: sample-ui-send-e2e\n1/1 Test #53: sample-ui-send-e2e ................***Failed   47.15 sec\n[sample-ui-send-e2e] waiting for sample-ui debug state\n[hosted_app_ui_e2e] browser errors before failure:\nBrowser page crashed.\n{\n  \"huge\": true,\n  \"nested\": {\n    \"more\": true\n  }\n}\nError: Failed while waiting for typed folder context attachment without draft mutation: page.evaluate: Target crashed .\n0% tests passed, 1 tests failed out of 1\nErrors while running CTest\n",
            8,
        );
        let filtered = filter_ctest_output(&output).unwrap();
        assert!(filtered.contains("[sample-ui-send-e2e] waiting for sample-ui debug state"));
        assert!(filtered.contains("[hosted_app_ui_e2e] browser errors before failure:"));
        assert!(filtered.contains("Browser page crashed."));
        assert!(
            filtered.contains("Error: Failed while waiting for typed folder context attachment")
        );
        assert!(!filtered.contains("\"huge\": true"));
    }

    #[cfg(unix)]
    #[test]
    fn run_passes_args_to_ctest() {
        let temp = tempdir().unwrap();
        let args_file = temp.path().join("ctest-args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '100%% tests passed, 0 tests failed out of 1\\n'\n",
            args_file.display()
        );
        crate::support::test_support::with_fake_path(&[("ctest", &script)], || {
            let output = run(&[
                "--test-dir".to_string(),
                "build".to_string(),
                "--output-on-failure".to_string(),
                "-R".to_string(),
                "dock-tests".to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("ctest: ok"));
        });
        let args = fs::read_to_string(args_file).unwrap();
        assert!(args.contains("--test-dir"));
        assert!(args.contains("build"));
        assert!(args.contains("--output-on-failure"));
        assert!(args.contains("-R"));
    }

    #[cfg(unix)]
    #[test]
    fn run_preserves_ctest_dash_n_catalog_output() {
        let script = "#!/bin/sh\nprintf 'Test project build\\n  Test #71: sample-ui-chat-browser-collaboration-model-tests\\n  Test #72: sample-ui-chat-markdown-model-tests\\nTotal Tests: 2\\n'\n";
        crate::support::test_support::with_fake_path(&[("ctest", script)], || {
            let output = run(&[
                "--test-dir".to_string(),
                "build".to_string(),
                "-N".to_string(),
                "-R".to_string(),
                "sample-ui-chat-browser-collaboration-model-tests|sample-ui-chat-markdown-model-tests"
                    .to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(
                output.stdout,
                "Test project build\n  Test #71: sample-ui-chat-browser-collaboration-model-tests\n  Test #72: sample-ui-chat-markdown-model-tests\nTotal Tests: 2\n"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn run_preserves_ctest_json_catalog_output_exactly() {
        let json = format!(
            "{{\"kind\":\"ctestInfo\",\"tests\":[{}]}}\n",
            (0..100)
                .map(|index| format!("{{\"name\":\"test-{index:03}\"}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let script = format!("#!/bin/sh\ncat <<'EOF'\n{json}EOF\n");
        crate::support::test_support::with_fake_path(&[("ctest", &script)], || {
            let output = run(&["--show-only=json-v1".to_string()]).unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(output.stdout, json);
        });
    }

    #[cfg(unix)]
    #[test]
    fn run_inherits_ctest_parallel_level_from_env() {
        let temp = tempdir().unwrap();
        let env_file = temp.path().join("ctest-env.txt");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"${{CTEST_PARALLEL_LEVEL:-unset}}\" > '{}'\nprintf '100%% tests passed, 0 tests failed out of 1\\n'\n",
            env_file.display()
        );
        crate::support::test_support::write_executable(Path::new(&bin_dir), "ctest", &script);
        crate::support::test_support::with_env_vars(
            &[
                ("CTEST_PARALLEL_LEVEL", Some("1")),
                ("PATH", Some(bin_dir.to_string_lossy().as_ref())),
                ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
                ("CX_TOOL_FALLBACK_PATHS", None),
            ],
            || {
                let output = run(&["--test-dir".to_string(), "build".to_string()]).unwrap();
                assert_eq!(output.exit_code, 0);
            },
        );
        assert_eq!(fs::read_to_string(env_file).unwrap().trim(), "1");
    }

    #[cfg(unix)]
    #[test]
    fn run_inherits_extra_query_env_var() {
        const EXTRA_QUERY_ENV: &str = "APP_E2E_EXTRA_QUERY";
        const EXTRA_QUERY_VALUE: &str = "sample-disable-webgl-app-tick=1";
        let temp = tempdir().unwrap();
        let env_file = temp.path().join("ctest-extra-query.txt");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"${{{}:-unset}}\" > '{}'\nprintf '100%% tests passed, 0 tests failed out of 1\\n'\n",
            EXTRA_QUERY_ENV,
            env_file.display()
        );
        crate::support::test_support::write_executable(Path::new(&bin_dir), "ctest", &script);
        crate::support::test_support::with_env_vars(
            &[
                (EXTRA_QUERY_ENV, Some(EXTRA_QUERY_VALUE)),
                ("PATH", Some(bin_dir.to_string_lossy().as_ref())),
                ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
                ("CX_TOOL_FALLBACK_PATHS", None),
            ],
            || {
                let output = run(&["--test-dir".to_string(), "build".to_string()]).unwrap();
                assert_eq!(output.exit_code, 0);
            },
        );
        assert_eq!(
            fs::read_to_string(env_file).unwrap().trim(),
            EXTRA_QUERY_VALUE
        );
    }
}
