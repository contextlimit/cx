use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;

use crate::support::redaction;
use crate::support::runner::{append_failure_hint, capture, ProxyOutcome};
use crate::support::utils::{fallback_window, resolved_command, truncate};

const EXACT_OUTPUT_MAX_LINES: usize = 80;
const MAX_GROUP_ROWS: usize = 25;
const MAX_PID_SAMPLES: usize = 4;
const MAX_EXAMPLE_CHARS: usize = 180;
const MAX_OTHER_EXECUTABLE_CHARS: usize = 1_600;

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    let no_compact = args.iter().any(|arg| arg == "--no-compact");
    let native_args = args
        .iter()
        .filter(|arg| arg.as_str() != "--no-compact")
        .cloned()
        .collect::<Vec<_>>();
    let exact_query = no_compact || has_pid_selector(&native_args);

    let mut command = resolved_command("ps");
    command.args(&native_args);
    let mut output = capture(command, "ps")?;

    if output.exit_code != 0 {
        let hint = output.failure_artifact_hint("ps");
        let observation = output.observation("ps");
        let stdout = if exact_query {
            output.stdout.trim_end().to_string()
        } else {
            fallback_window(&output.stdout, 12, 28)
        };
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(stdout, hint.as_deref()),
            stderr: output.stderr.trim_end().to_string(),
            exit_code: output.exit_code,
            observation: None,
        }
        .with_observation(observation));
    }

    let (stdout, compacted) = render_success(&output.stdout, &native_args, exact_query);
    let outcome = ProxyOutcome {
        stdout,
        stderr: output.stderr.trim_end().to_string(),
        exit_code: output.exit_code,
        observation: None,
    }
    .with_observation(output.observation("ps"));
    Ok(if compacted {
        outcome.with_expansion_reason("process-inventory-summary")
    } else {
        outcome
    })
}

fn render_success(raw: &str, args: &[String], exact_query: bool) -> (String, bool) {
    let raw = raw.trim_end();
    let line_count = raw.lines().filter(|line| !line.trim().is_empty()).count();
    if raw.is_empty() || exact_query || line_count <= EXACT_OUTPUT_MAX_LINES {
        return (raw.to_string(), false);
    }
    let output = summarize_process_inventory(raw, args).unwrap_or_else(|| {
        format!(
            "ps: {} output lines (unstructured)\n{}\n{}",
            line_count,
            fallback_window(raw, 12, 28),
            full_table_hint(args),
        )
    });
    (output, true)
}

fn has_pid_selector(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(arg.as_str(), "-p" | "--pid" | "-q" | "--quick-pid")
            || arg.starts_with("--pid=")
            || arg.starts_with("--quick-pid=")
            || (arg.starts_with("-p") && arg.len() > 2)
            || (arg.starts_with("-q") && arg.len() > 2)
            || is_pid_list(arg)
    })
}

fn is_pid_list(value: &str) -> bool {
    !value.is_empty()
        && value
            .split(',')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn summarize_process_inventory(raw: &str, args: &[String]) -> Option<String> {
    let lines = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let first = *lines.first()?;
    let layout = ProcessLayout::detect(args, first)?;
    let skip_header = layout.first_line_is_header(first);
    let process_lines = if skip_header { &lines[1..] } else { &lines[..] };
    let mut groups = BTreeMap::<String, ProcessGroup>::new();
    let mut parsed_rows = 0usize;

    for line in process_lines {
        let Some(fields) = split_columns(line, layout.column_count) else {
            continue;
        };
        let command = fields.get(layout.command_index)?.trim();
        if command.is_empty() {
            continue;
        }
        let pid = layout
            .pid_index
            .and_then(|index| fields.get(index))
            .map(|value| value.trim())
            .filter(|value| is_pid_list(value));
        let label = redacted_executable_label(command);
        let group = groups.entry(label).or_default();
        group.record(pid, command);
        parsed_rows += 1;
    }

    if parsed_rows == 0 || parsed_rows.saturating_mul(10) < process_lines.len().saturating_mul(9) {
        return None;
    }

    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        right
            .1
            .count
            .cmp(&left.1.count)
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut result = format!(
        "ps: {} processes across {} executables\nprocess | count | sample pids | example\n",
        parsed_rows,
        groups.len()
    );
    for (label, group) in groups.iter().take(MAX_GROUP_ROWS) {
        result.push_str(&format!(
            "{} | {} | {} | {}\n",
            truncate(label, 60),
            group.count,
            group.pid_summary(),
            group.example,
        ));
    }

    if groups.len() > MAX_GROUP_ROWS {
        let mut remaining = groups
            .iter()
            .skip(MAX_GROUP_ROWS)
            .map(|(label, _)| label.as_str())
            .collect::<Vec<_>>();
        remaining.sort_unstable();
        result.push_str(&format!(
            "other executables ({}): {}\n",
            remaining.len(),
            truncate(&remaining.join(", "), MAX_OTHER_EXECUTABLE_CHARS,),
        ));
    }
    result.push_str(&full_table_hint(args));
    Some(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessLayout {
    column_count: usize,
    pid_index: Option<usize>,
    command_index: usize,
    custom_columns: bool,
}

impl ProcessLayout {
    fn detect(args: &[String], first_line: &str) -> Option<Self> {
        let custom_names = custom_output_columns(args);
        if !custom_names.is_empty() {
            return Self::from_names(&custom_names, true);
        }
        let header_names = first_line
            .split_whitespace()
            .map(normalize_column_name)
            .collect::<Vec<_>>();
        Self::from_names(&header_names, false)
    }

    fn from_names(names: &[String], custom_columns: bool) -> Option<Self> {
        let command_index = names.iter().position(|name| is_command_column(name))?;
        if command_index + 1 != names.len() {
            return None;
        }
        Some(Self {
            column_count: names.len(),
            pid_index: names.iter().position(|name| name == "pid"),
            command_index,
            custom_columns,
        })
    }

    fn first_line_is_header(&self, first_line: &str) -> bool {
        let Some(fields) = split_columns(first_line, self.column_count) else {
            return false;
        };
        if let Some(pid_index) = self.pid_index {
            return fields
                .get(pid_index)
                .is_some_and(|value| !is_pid_list(value.trim()));
        }
        if !self.custom_columns {
            return true;
        }
        fields
            .get(self.command_index)
            .is_some_and(|value| is_command_column(&normalize_column_name(value)))
    }
}

fn custom_output_columns(args: &[String]) -> Vec<String> {
    let mut specs = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if matches!(arg.as_str(), "-o" | "--format") || short_flags_consume_format(arg) {
            if let Some(spec) = args.get(index + 1) {
                specs.push(spec.as_str());
                index += 2;
                continue;
            }
        } else if let Some(spec) = arg.strip_prefix("--format=") {
            specs.push(spec);
        } else if let Some(spec) = arg.strip_prefix("-o").filter(|spec| !spec.is_empty()) {
            specs.push(spec);
        }
        index += 1;
    }
    specs
        .into_iter()
        .flat_map(|spec| spec.split(|ch: char| ch == ',' || ch.is_whitespace()))
        .filter(|name| !name.is_empty())
        .map(normalize_column_name)
        .collect()
}

fn short_flags_consume_format(arg: &str) -> bool {
    let Some(flags) = arg.strip_prefix('-') else {
        return false;
    };
    !arg.starts_with("--") && flags.len() > 1 && flags.ends_with('o')
}

fn normalize_column_name(name: &str) -> String {
    name.trim()
        .split_once('=')
        .map_or(name.trim(), |(name, _)| name)
        .trim_start_matches('%')
        .to_ascii_lowercase()
}

fn is_command_column(name: &str) -> bool {
    matches!(name, "command" | "cmd" | "args" | "comm" | "ucmd")
}

fn split_columns(line: &str, column_count: usize) -> Option<Vec<&str>> {
    if column_count == 0 {
        return None;
    }
    let mut fields = Vec::with_capacity(column_count);
    let mut rest = line.trim_start();
    for _ in 1..column_count {
        let split_at = rest.find(char::is_whitespace)?;
        fields.push(&rest[..split_at]);
        rest = rest[split_at..].trim_start();
    }
    fields.push(rest);
    Some(fields)
}

fn executable_label(command: &str) -> String {
    let executable = command.split_whitespace().next().unwrap_or("<unknown>");
    Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(executable)
        .to_string()
}

fn redacted_executable_label(command: &str) -> String {
    let label = executable_label(command);
    redaction::redact_argv(&[label])
        .into_iter()
        .next()
        .unwrap_or_else(|| "<unknown>".to_string())
}

#[derive(Debug, Default)]
struct ProcessGroup {
    count: usize,
    pids: Vec<String>,
    example: String,
}

impl ProcessGroup {
    fn record(&mut self, pid: Option<&str>, command: &str) {
        self.count += 1;
        if let Some(pid) = pid {
            if self.pids.len() < MAX_PID_SAMPLES && !self.pids.iter().any(|value| value == pid) {
                self.pids.push(pid.to_string());
            }
        }
        if self.example.is_empty() {
            let argv = command
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            self.example = truncate(&redaction::redacted_shell_join(&argv), MAX_EXAMPLE_CHARS);
        }
    }

    fn pid_summary(&self) -> String {
        if self.pids.is_empty() {
            "-".to_string()
        } else if self.count > self.pids.len() {
            format!("{}, +{}", self.pids.join(","), self.count - self.pids.len())
        } else {
            self.pids.join(",")
        }
    }
}

fn full_table_hint(args: &[String]) -> String {
    let mut command = vec![
        "cx".to_string(),
        "--".to_string(),
        "ps".to_string(),
        "--no-compact".to_string(),
    ];
    command.extend(
        args.iter()
            .filter(|arg| arg.as_str() != "--no-compact")
            .cloned(),
    );
    format!(
        "[full process table: {}]",
        redaction::redacted_shell_join(&command)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_summary_groups_processes_and_preserves_catalog() {
        let mut raw = String::from("PID PPID ELAPSED COMMAND\n");
        for pid in 1..=100 {
            raw.push_str(&format!("{pid} 1 00:01 /usr/bin/node app-{pid}.mjs\n"));
        }
        raw.push_str("501 1 00:03 /opt/tools/rare-service --token=sk-secretsecret\n");
        raw.push_str("502 1 00:04 /tmp/sk-secretsecret --serve\n");

        let summary = summarize_process_inventory(
            &raw,
            &["-axo".to_string(), "pid,ppid,etime,command".to_string()],
        )
        .unwrap();
        assert!(summary.contains("ps: 102 processes across 3 executables"));
        assert!(summary.contains("node | 100 | 1,2,3,4, +96"));
        assert!(summary.contains("rare-service"));
        assert!(summary.contains("[REDACTED] | 1"));
        assert!(summary.contains("[full process table: cx -- ps --no-compact"));
    }

    #[test]
    fn headerless_custom_columns_are_parsed() {
        let raw = "101 1 00:01 /usr/bin/node app.mjs\n102 1 00:02 /usr/bin/node worker.mjs\n";
        let summary = summarize_process_inventory(
            raw,
            &["-axo".to_string(), "pid=,ppid=,etime=,command=".to_string()],
        )
        .unwrap();
        assert!(summary.contains("ps: 2 processes across 1 executables"));
        assert!(summary.contains("node | 2 | 101,102"));
    }

    #[test]
    fn pid_selector_detection_covers_common_shapes() {
        assert!(has_pid_selector(&["-p".to_string(), "123".to_string()]));
        assert!(has_pid_selector(&["--pid=123,456".to_string()]));
        assert!(has_pid_selector(&["123,456".to_string()]));
        assert!(!has_pid_selector(&["aux".to_string()]));
        assert!(!has_pid_selector(&[
            "-axo".to_string(),
            "pid,command".to_string()
        ]));
    }

    #[cfg(unix)]
    #[test]
    fn narrow_and_no_compact_queries_preserve_exact_output() {
        crate::support::test_support::with_fake_path(
            &[(
                "ps",
                "#!/bin/sh\ncase \" $* \" in *\" --no-compact \"*) exit 9 ;; esac\nprintf 'PID COMMAND\\n101 /usr/bin/node app.mjs\\n'\n",
            )],
            || {
                let narrow = run(&["-p".to_string(), "101".to_string()]).unwrap();
                assert_eq!(narrow.stdout, "PID COMMAND\n101 /usr/bin/node app.mjs");
                let full = run(&[
                    "--no-compact".to_string(),
                    "-axo".to_string(),
                    "pid,command".to_string(),
                ])
                .unwrap();
                assert_eq!(full.stdout, narrow.stdout);
            },
        );
    }
}
