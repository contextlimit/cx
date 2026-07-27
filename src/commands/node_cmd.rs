use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::support::command_output::{self, OutputProfile};
use crate::support::command_repair;
use crate::support::insights::{self, OutputObservation, TextMetrics};
use crate::support::redaction;
use crate::support::runner::{
    append_failure_hint, capture, capture_with_piped_stdin, failure_artifact_hint, CommandOutput,
    ProxyOutcome,
};
use crate::support::utils::resolved_command;

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    match parse_node_invocation(args)? {
        NodeInvocation::Check(check) => run_check(check),
        NodeInvocation::Run(node_args) => run_node_runtime("node run", node_args, None),
        NodeInvocation::Test(test_args) => {
            let mut node_args = Vec::with_capacity(test_args.len() + 1);
            node_args.push("--test".to_string());
            node_args.extend(test_args);
            run_node_runtime("node test", node_args, Some(OutputProfile::NodeTest))
        }
    }
}

pub fn command_label(args: &[String]) -> &'static str {
    match args.first().map(String::as_str) {
        None => "node run",
        Some("run") => "node run",
        Some("test") => "node test",
        Some(arg) if is_stdin_runtime_arg(arg) => "node run",
        _ if check_flag_precedes_program(args) => "node check",
        _ => "node run",
    }
}

pub(crate) fn check_flag_precedes_program(args: &[String]) -> bool {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--check" | "-c" => return true,
            "--experimental-loader" | "--loader" => {
                index += 2;
            }
            value
                if value.starts_with("--experimental-loader=")
                    || value.starts_with("--loader=") =>
            {
                index += 1;
            }
            _ => return false,
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeInvocation {
    Check(NodeCheckArgs),
    Run(Vec<String>),
    Test(Vec<String>),
}

fn parse_node_invocation(args: &[String]) -> Result<NodeInvocation> {
    match args.first().map(String::as_str) {
        None => Ok(NodeInvocation::Run(Vec::new())),
        Some("run") => parse_run_args(&args[1..]).map(NodeInvocation::Run),
        Some("test") => Ok(NodeInvocation::Test(args[1..].to_vec())),
        Some(arg) if is_stdin_runtime_arg(arg) => Ok(NodeInvocation::Run(args.to_vec())),
        _ => parse_check_args(args).map(NodeInvocation::Check),
    }
}

fn parse_run_args(args: &[String]) -> Result<Vec<String>> {
    if args.is_empty() {
        anyhow::bail!("`cx node run` requires node arguments or a script path");
    }
    Ok(args.to_vec())
}

fn is_stdin_runtime_arg(arg: &str) -> bool {
    arg == "--input-type" || arg.starts_with("--input-type=")
}

fn run_check(check: NodeCheckArgs) -> Result<ProxyOutcome> {
    let mut success_lines = Vec::new();
    let mut failure_blocks = Vec::new();
    let mut exit_code = 0;
    let mut raw_metrics = TextMetrics::default();

    for file in &check.files {
        let outcome = if is_jsx_file(file) {
            check_jsx_file(file)?
        } else {
            check_with_node(file)?
        };
        raw_metrics = raw_metrics.plus(
            outcome
                .observation
                .as_ref()
                .map(|observation| observation.metrics)
                .unwrap_or_else(|| TextMetrics::from_text(&outcome.stdout)),
        );

        if outcome.exit_code == 0 {
            if !outcome.stdout.trim().is_empty() {
                success_lines.push(outcome.stdout.trim_end().to_string());
            }
            continue;
        }

        exit_code = outcome.exit_code;
        let mut block = String::new();
        if !outcome.stdout.trim().is_empty() {
            block.push_str(outcome.stdout.trim_end());
        }
        if !outcome.stderr.trim().is_empty() {
            if !block.is_empty() {
                block.push('\n');
            }
            block.push_str(outcome.stderr.trim_end());
        }
        if block.is_empty() {
            block = format!("node --check failed ({})", file.display());
        }
        failure_blocks.push(block);
    }

    Ok(ProxyOutcome {
        stdout: success_lines.join("\n"),
        stderr: failure_blocks.join("\n\n"),
        exit_code,
        observation: None,
    }
    .with_observation(OutputObservation::from_metrics("node check", raw_metrics))
    .with_expansion_reason("syntax-check-summary"))
}

fn run_node_runtime(
    source: &'static str,
    node_args: Vec<String>,
    output_profile: Option<OutputProfile>,
) -> Result<ProxyOutcome> {
    if node_args.is_empty() && io::stdin().is_terminal() {
        bail!("`cx node` without arguments requires piped stdin");
    }
    let mut cmd = resolved_command("node");
    cmd.args(&node_args);
    let mut output = capture_node_runtime(cmd)?;
    let failure_hint = if output.exit_code == 0 {
        None
    } else {
        output.failure_artifact_hint("node")
    };
    let observation = output.observation(source);
    let optimizations_enabled = insights::command_optimizations_enabled().unwrap_or(true);
    let mut compacted = if optimizations_enabled {
        output_profile.map(|profile| {
            command_output::compact_streams(
                profile,
                &output.stdout,
                &output.stderr,
                output.exit_code,
            )
        })
    } else {
        None
    };
    let mut stdout = compacted
        .as_mut()
        .and_then(|streams| streams.stdout.take())
        .unwrap_or_else(|| std::mem::take(&mut output.stdout));
    let mut stderr = compacted
        .as_mut()
        .and_then(|streams| streams.stderr.take())
        .unwrap_or_else(|| std::mem::take(&mut output.stderr));

    if output.exit_code != 0 {
        stdout = append_failure_hint(stdout, failure_hint.as_deref());
        append_node_repair_advice(&mut stderr, &node_args, output.exit_code, &output.combined);
    }

    Ok(ProxyOutcome {
        stdout,
        stderr,
        exit_code: output.exit_code,
        observation: None,
    }
    .with_observation(observation))
}

fn capture_node_runtime(cmd: std::process::Command) -> Result<CommandOutput> {
    capture_with_piped_stdin(cmd, "node")
}

fn append_node_repair_advice(stderr: &mut String, args: &[String], exit_code: i32, output: &str) {
    let Some(advice) = command_repair::node_runtime_advice(args, exit_code, output) else {
        return;
    };
    command_repair::append_note(stderr, &advice.note);
    record_node_repair_advice(args, exit_code, output, &advice);
}

fn record_node_repair_advice(
    args: &[String],
    exit_code: i32,
    output: &str,
    advice: &command_repair::CommandAdvice,
) {
    let mut command_args = Vec::with_capacity(args.len() + 1);
    command_args.push("node".to_string());
    command_args.extend(args.iter().cloned());
    let command = redaction::redacted_shell_join(&command_args);
    let record = insights::CommandRepairRecord {
        process: "node",
        command_family: "node run",
        command: &command,
        source: "node run",
        rule_id: advice.rule_id,
        action: "advice",
        original_exit_code: exit_code,
        final_exit_code: exit_code,
        original_response: output,
        final_response: output,
    };
    let _ = insights::record_command_repair(&record);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeCheckArgs {
    files: Vec<PathBuf>,
}

fn parse_check_args(args: &[String]) -> Result<NodeCheckArgs> {
    let mut check_flag_seen = false;
    let mut file_seen_before_check = false;
    let mut files = Vec::new();
    let mut after_separator = false;
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if after_separator {
            if !check_flag_seen {
                file_seen_before_check = true;
            }
            files.push(PathBuf::from(arg));
            index += 1;
            continue;
        }

        match arg.as_str() {
            "--" => {
                after_separator = true;
                index += 1;
            }
            "--check" | "-c" => {
                check_flag_seen = true;
                index += 1;
            }
            "--experimental-loader" | "--loader" => {
                index += 1;
                if args.get(index).is_none() {
                    anyhow::bail!("`{}` requires a loader path", arg);
                }
                index += 1;
            }
            value
                if value.starts_with("--experimental-loader=")
                    || value.starts_with("--loader=") =>
            {
                index += 1;
            }
            value if value.starts_with('-') => {
                anyhow::bail!(
                    "`cx node` only supports `--check <file>` and `-c <file>`; unsupported argument `{value}`"
                );
            }
            value => {
                if !check_flag_seen {
                    file_seen_before_check = true;
                }
                files.push(PathBuf::from(value));
                index += 1;
            }
        }
    }

    if check_flag_seen && file_seen_before_check {
        anyhow::bail!(
            "`--check` and `-c` must precede file paths; use `cx -- node <script> --check` when `--check` is an argument for the script"
        );
    }

    if !check_flag_seen {
        anyhow::bail!("`cx node` only supports `--check <file>` and `-c <file>`");
    }

    if files.is_empty() {
        anyhow::bail!("`cx node --check` requires at least one file path");
    }

    Ok(NodeCheckArgs { files })
}

fn check_with_node(file: &Path) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("node");
    cmd.arg("--check");
    cmd.arg(file);

    let mut output = capture(cmd, "node")?;
    if output.exit_code == 0 {
        let observation = output.observation("node --check");
        return Ok(
            ProxyOutcome::success(format!("node --check: syntax ok ({})", file.display()))
                .with_observation(observation),
        );
    }

    let failure_hint = output.failure_artifact_hint("node");
    let observation = output.observation("node --check");
    let stdout = std::mem::take(&mut output.stdout);
    let stderr = std::mem::take(&mut output.stderr);
    Ok(ProxyOutcome {
        stdout: if stdout.trim().is_empty() {
            String::new()
        } else {
            stdout.trim_end().to_string()
        },
        stderr: append_failure_hint(stderr.trim_end().to_string(), failure_hint.as_deref()),
        exit_code: output.exit_code,
        observation: None,
    }
    .with_observation(observation))
}

fn check_jsx_file(file: &Path) -> Result<ProxyOutcome> {
    let source = fs::read_to_string(file)
        .with_context(|| format!("failed to read JSX source {}", file.display()))?;
    let allocator = Allocator::default();
    let source_type = source_type_for_jsx(file);
    let parsed = Parser::new(&allocator, &source, source_type).parse();

    if parsed.panicked || !parsed.errors.is_empty() {
        let error = parsed
            .errors
            .first()
            .map(|diagnostic| {
                let offset = diagnostic
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.first())
                    .map(|label| label.offset())
                    .unwrap_or(0);
                format_jsx_error(file, &source, offset, diagnostic)
            })
            .unwrap_or_else(|| format!("{}: JSX parse failed", file.display()));
        let failure_hint = failure_artifact_hint("node", "", &error);
        let outcome = ProxyOutcome {
            stdout: String::new(),
            stderr: append_failure_hint(error, failure_hint.as_deref()),
            exit_code: 1,
            observation: None,
        }
        .with_raw_output("jsx source", &source);
        return Ok(if failure_hint.is_some() {
            outcome.with_expansion_reason(crate::support::runner::FAILURE_ARTIFACT_EXPANSION_REASON)
        } else {
            outcome
        });
    }

    Ok(ProxyOutcome::success(format!(
        "node --check: syntax ok ({}) [jsx parser]",
        file.display()
    ))
    .with_raw_output("jsx source", &source))
}

fn source_type_for_jsx(file: &Path) -> SourceType {
    let mut source_type = SourceType::default().with_jsx(true);
    if matches!(
        file.extension().and_then(|extension| extension.to_str()),
        Some("mjs" | "jsx")
    ) {
        source_type = source_type.with_module(true);
    }
    source_type
}

fn format_jsx_error(
    file: &Path,
    source: &str,
    offset: usize,
    diagnostic: &impl std::fmt::Display,
) -> String {
    let (line, column) = offset_to_line_column(source, offset);
    format!("{}:{}:{}: {}", file.display(), line, column, diagnostic)
}

fn offset_to_line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (index, ch) in source.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn is_jsx_file(file: &Path) -> bool {
    matches!(
        file.extension().and_then(|extension| extension.to_str()),
        Some("jsx")
    )
}

#[cfg(test)]
mod tests;
