use anyhow::Result;

use crate::support::insights::{OutputObservation, TextMetrics};
use crate::support::runner::{run_filtered, ProxyOutcome, RunOptions};
use crate::support::utils::{resolved_command, truncate};

#[derive(Debug, Default)]
struct CompileDiagnostic {
    header: String,
    location: String,
    span: Vec<String>,
}

#[derive(Debug, Default)]
struct CompileFailure {
    diagnostics: Vec<CompileDiagnostic>,
    trailing: Vec<String>,
}

pub fn run_test(args: &[String]) -> Result<ProxyOutcome> {
    let restored_args = restore_double_dash(args);
    if let Some(plan) = SplitFilterTestPlan::from_args(&restored_args) {
        return run_split_filter_tests(&plan);
    }

    run_single_test(&restored_args)
}

fn run_single_test(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("cargo");
    cmd.arg("test");
    for arg in args {
        cmd.arg(arg);
    }

    run_filtered(
        cmd,
        "cargo",
        |output| Some(filter_cargo_test(&output.combined)),
        RunOptions::default(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SplitFilterTestPlan {
    prefix_args: Vec<String>,
    filters: Vec<String>,
    harness_args: Vec<String>,
    had_separator: bool,
}

impl SplitFilterTestPlan {
    fn from_args(args: &[String]) -> Option<Self> {
        let (cargo_args, harness_args, had_separator) = split_cargo_and_harness_args(args);
        let (prefix_args, filters) = split_prefix_and_filters(cargo_args)?;
        if filters.len() < 2 {
            return None;
        }
        Some(Self {
            prefix_args,
            filters,
            harness_args,
            had_separator,
        })
    }

    fn args_for_filter(&self, filter: &str) -> Vec<String> {
        let mut args = Vec::with_capacity(
            self.prefix_args.len() + 1 + usize::from(self.had_separator) + self.harness_args.len(),
        );
        args.extend(self.prefix_args.iter().cloned());
        args.push(filter.to_string());
        if self.had_separator {
            args.push("--".to_string());
            args.extend(self.harness_args.iter().cloned());
        }
        args
    }
}

fn run_split_filter_tests(plan: &SplitFilterTestPlan) -> Result<ProxyOutcome> {
    let mut stdout = format!(
        "cargo test: split {} filters into {} cargo test runs\n",
        plan.filters.len(),
        plan.filters.len()
    );
    let mut stderr = String::new();
    let mut exit_code = 0;
    let mut raw_metrics = TextMetrics::default();

    for (index, filter) in plan.filters.iter().enumerate() {
        let args = plan.args_for_filter(filter);
        let outcome = run_single_test(&args)?;
        if outcome.exit_code != 0 && exit_code == 0 {
            exit_code = outcome.exit_code;
        }
        if let Some(observation) = &outcome.observation {
            raw_metrics = raw_metrics.plus(observation.metrics);
        }
        stdout.push_str(&format!(
            "[{}/{}] {} (exit {})\n",
            index + 1,
            plan.filters.len(),
            filter,
            outcome.exit_code
        ));
        if !outcome.stdout.trim().is_empty() {
            stdout.push_str(outcome.stdout.trim());
            stdout.push('\n');
        }
        if !outcome.stderr.trim().is_empty() {
            stderr.push_str(outcome.stderr.trim());
            stderr.push('\n');
        }
    }

    Ok(ProxyOutcome {
        stdout: stdout.trim_end().to_string(),
        stderr: stderr.trim_end().to_string(),
        exit_code,
        observation: None,
    }
    .with_observation(OutputObservation::from_metrics(
        "cargo test split-filters",
        raw_metrics,
    )))
}

fn split_cargo_and_harness_args(args: &[String]) -> (&[String], Vec<String>, bool) {
    if let Some(separator) = args.iter().position(|arg| arg == "--") {
        (&args[..separator], args[(separator + 1)..].to_vec(), true)
    } else {
        (args, Vec::new(), false)
    }
}

fn split_prefix_and_filters(args: &[String]) -> Option<(Vec<String>, Vec<String>)> {
    let mut prefix_args = Vec::new();
    let mut filters = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if filters.is_empty() && cargo_test_option_takes_value(arg) {
            prefix_args.push(arg.clone());
            index += 1;
            let value = args.get(index)?;
            prefix_args.push(value.clone());
        } else if filters.is_empty()
            && (cargo_test_option_with_attached_value(arg) || cargo_test_flag_without_value(arg))
        {
            prefix_args.push(arg.clone());
        } else if arg.starts_with('-') {
            return None;
        } else {
            filters.push(arg.clone());
        }
        index += 1;
    }

    Some((prefix_args, filters))
}

fn cargo_test_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-p" | "--package"
            | "--exclude"
            | "--bin"
            | "--example"
            | "--test"
            | "--bench"
            | "--target"
            | "--target-dir"
            | "--manifest-path"
            | "--features"
            | "-j"
            | "--jobs"
            | "--message-format"
            | "--color"
            | "--profile"
            | "--config"
            | "-Z"
            | "--lockfile-path"
    )
}

fn cargo_test_option_with_attached_value(arg: &str) -> bool {
    const LONG_PREFIXES: &[&str] = &[
        "--package=",
        "--exclude=",
        "--bin=",
        "--example=",
        "--test=",
        "--bench=",
        "--target=",
        "--target-dir=",
        "--manifest-path=",
        "--features=",
        "--jobs=",
        "--message-format=",
        "--color=",
        "--profile=",
        "--config=",
        "--timings=",
        "--lockfile-path=",
    ];
    LONG_PREFIXES.iter().any(|prefix| arg.starts_with(prefix))
        || short_option_with_attached_value(arg)
}

fn short_option_with_attached_value(arg: &str) -> bool {
    matches!(arg.as_bytes(), [b'-', b'p' | b'j' | b'Z', ..] if arg.len() > 2)
}

fn cargo_test_flag_without_value(arg: &str) -> bool {
    matches!(
        arg,
        "--workspace"
            | "--all"
            | "--lib"
            | "--bins"
            | "--examples"
            | "--tests"
            | "--benches"
            | "--all-targets"
            | "--doc"
            | "--no-run"
            | "--no-fail-fast"
            | "--release"
            | "--all-features"
            | "--no-default-features"
            | "--locked"
            | "--offline"
            | "--frozen"
            | "--ignore-rust-version"
            | "--verbose"
            | "--quiet"
            | "--future-incompat-report"
            | "--keep-going"
            | "--unit-graph"
            | "--timings"
            | "-v"
            | "-q"
    ) || short_verbosity_cluster(arg)
}

fn short_verbosity_cluster(arg: &str) -> bool {
    arg.len() > 2
        && arg.starts_with('-')
        && !arg.starts_with("--")
        && arg.chars().skip(1).all(|value| matches!(value, 'v' | 'q'))
}

fn restore_double_dash(args: &[String]) -> Vec<String> {
    let raw_args: Vec<String> = std::env::args().collect();
    restore_double_dash_with_raw(args, &raw_args)
}

fn restore_double_dash_with_raw(args: &[String], raw_args: &[String]) -> Vec<String> {
    if args.is_empty() || args.iter().any(|arg| arg == "--") {
        return args.to_vec();
    }

    let Some(cargo_test_args_start) = cargo_test_args_start(raw_args) else {
        return args.to_vec();
    };
    let Some(separator_offset) = raw_args[cargo_test_args_start..]
        .iter()
        .position(|arg| arg == "--")
    else {
        return args.to_vec();
    };
    let args_before_separator = separator_offset.min(args.len());

    let mut result = Vec::with_capacity(args.len() + 1);
    result.extend_from_slice(&args[..args_before_separator]);
    result.push("--".to_string());
    result.extend_from_slice(&args[args_before_separator..]);
    result
}

fn cargo_test_args_start(raw_args: &[String]) -> Option<usize> {
    raw_args
        .windows(2)
        .position(|window| window == ["cargo", "test"])
        .map(|index| index + 2)
}

fn filter_cargo_test(output: &str) -> String {
    let mut failures = Vec::new();
    let mut summary_lines = Vec::new();
    let mut in_failure_section = false;
    let mut current_failure = Vec::new();

    for line in output.lines() {
        if line.trim_start().starts_with("Compiling")
            || line.trim_start().starts_with("Downloading")
            || line.trim_start().starts_with("Downloaded")
            || line.trim_start().starts_with("Finished")
        {
            continue;
        }
        if line.starts_with("running ") || (line.starts_with("test ") && line.ends_with("... ok")) {
            continue;
        }
        if line == "failures:" {
            in_failure_section = true;
            continue;
        }
        if in_failure_section {
            if line.starts_with("test result:") {
                in_failure_section = false;
                summary_lines.push(line.to_string());
            } else if line.starts_with("    ") || line.starts_with("---- ") {
                current_failure.push(line.to_string());
            } else if line.trim().is_empty() && !current_failure.is_empty() {
                failures.push(current_failure.join("\n"));
                current_failure.clear();
            } else if !line.trim().is_empty() {
                current_failure.push(line.to_string());
            }
        }
        if !in_failure_section && line.starts_with("test result:") {
            summary_lines.push(line.to_string());
        }
    }

    if !current_failure.is_empty() {
        failures.push(current_failure.join("\n"));
    }

    if failures.is_empty() && summary_lines.is_empty() {
        if let Some(compile_failure) = filter_cargo_compile_failure(output) {
            return compile_failure;
        }
    }

    if failures.is_empty() && !summary_lines.is_empty() {
        return summary_lines.join("\n").trim().to_string();
    }

    let mut result = String::new();
    if !failures.is_empty() {
        result.push_str(&format!("FAILURES ({}):\n", failures.len()));
        result.push_str("═══════════════════════════════════════\n");
        for (index, failure) in failures.iter().enumerate().take(10) {
            result.push_str(&format!("{}. {}\n", index + 1, truncate(failure, 200)));
        }
        if failures.len() > 10 {
            result.push_str(&format!("\n... +{} more failures\n", failures.len() - 10));
        }
        result.push('\n');
    }

    for line in &summary_lines {
        result.push_str(line);
        result.push('\n');
    }

    if result.trim().is_empty() {
        let meaningful: Vec<&str> = output
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("Compiling"))
            .collect();
        for line in meaningful.iter().rev().take(5).rev() {
            result.push_str(line);
            result.push('\n');
        }
    }

    result.trim().to_string()
}

fn filter_cargo_compile_failure(output: &str) -> Option<String> {
    if !looks_like_compile_failure(output) {
        return None;
    }

    let failure = collect_compile_failure(output);
    if failure.diagnostics.is_empty() && failure.trailing.is_empty() {
        return None;
    }
    Some(render_compile_failure(&failure))
}

fn looks_like_compile_failure(output: &str) -> bool {
    output.contains("could not compile") || output.contains("error[") || output.contains("error:")
}

fn collect_compile_failure(output: &str) -> CompileFailure {
    let lines: Vec<&str> = output.lines().collect();
    let mut failure = CompileFailure::default();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index].trim_end();
        let trimmed = line.trim();

        if is_compile_error_header(trimmed) {
            failure.diagnostics.push(parse_compile_diagnostic(
                &lines,
                &mut index,
                &mut failure.trailing,
            ));
            continue;
        }

        if trimmed.starts_with("warning:") || trimmed.starts_with("error: could not compile") {
            failure.trailing.push(trimmed.to_string());
        }
        index += 1;
    }

    failure
}

fn parse_compile_diagnostic(
    lines: &[&str],
    index: &mut usize,
    trailing: &mut Vec<String>,
) -> CompileDiagnostic {
    let mut diagnostic = CompileDiagnostic {
        header: lines[*index].trim().to_string(),
        ..CompileDiagnostic::default()
    };
    *index += 1;

    while *index < lines.len() {
        let next = lines[*index].trim_end();
        let next_trimmed = next.trim();
        if is_compile_error_header(next_trimmed) {
            break;
        }
        if next_trimmed.is_empty() {
            *index += 1;
            if !diagnostic.location.is_empty() || !diagnostic.span.is_empty() {
                break;
            }
            continue;
        }
        if diagnostic.location.is_empty() && next_trimmed.starts_with("-->") {
            diagnostic.location = next_trimmed.to_string();
            *index += 1;
            continue;
        }
        if is_compile_trailing_line(next_trimmed) {
            trailing.push(next_trimmed.to_string());
            *index += 1;
            continue;
        }
        push_compile_span_line(&mut diagnostic, next_trimmed);
        *index += 1;
    }

    diagnostic
}

fn is_compile_trailing_line(line: &str) -> bool {
    line.starts_with("warning:")
        || line.starts_with("error: could not compile")
        || line.starts_with("For more information")
}

fn push_compile_span_line(diagnostic: &mut CompileDiagnostic, trimmed: &str) {
    if diagnostic.span.len() < 4 {
        diagnostic.span.push(trimmed.to_string());
    }
}

fn render_compile_failure(failure: &CompileFailure) -> String {
    let mut result = String::new();
    result.push_str(&format!(
        "cargo test: compile failed ({} diagnostics)\n",
        failure.diagnostics.len()
    ));
    result.push_str("═══════════════════════════════════════\n");

    for (index, diagnostic) in failure.diagnostics.iter().take(5).enumerate() {
        let (code, message) = split_compile_error_header(&diagnostic.header);
        if code.is_empty() {
            result.push_str(&format!("{}. {}\n", index + 1, truncate(&message, 140)));
        } else {
            result.push_str(&format!(
                "{}. [{}] {}\n",
                index + 1,
                code,
                truncate(&message, 140)
            ));
        }
        if !diagnostic.location.is_empty() {
            result.push_str(&format!("   {}\n", diagnostic.location));
        }
        for span in &diagnostic.span {
            result.push_str(&format!("   {}\n", truncate(span, 140)));
        }
    }
    if failure.diagnostics.len() > 5 {
        result.push_str(&format!(
            "... +{} more diagnostics\n",
            failure.diagnostics.len() - 5
        ));
    }

    for line in unique_trailing_lines(&failure.trailing).iter().take(3) {
        result.push_str(&format!("{}\n", truncate(line, 180)));
    }

    result.trim().to_string()
}

fn unique_trailing_lines(lines: &[String]) -> Vec<&String> {
    let mut unique = Vec::new();
    for line in lines {
        if !unique.contains(&line) {
            unique.push(line);
        }
    }
    unique
}

fn is_compile_error_header(line: &str) -> bool {
    line.starts_with("error[")
        || (line.starts_with("error:") && !line.starts_with("error: could not compile"))
}

fn split_compile_error_header(header: &str) -> (String, String) {
    if let Some(rest) = header.strip_prefix("error[") {
        if let Some((code, message)) = rest.split_once("]:") {
            return (code.to_string(), message.trim().to_string());
        }
    }
    (
        String::new(),
        header
            .strip_prefix("error:")
            .unwrap_or(header)
            .trim()
            .to_string(),
    )
}

#[cfg(test)]
mod tests;
