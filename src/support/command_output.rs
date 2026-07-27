use std::path::Path;

use crate::support::cmake_failure;
use crate::support::utils::fallback_window;

const MIN_COMPACT_LINES: usize = 60;
const MIN_COMPACT_BYTES: usize = 16_000;
const MAX_EVIDENCE_LINES: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputProfile {
    NodeTest,
    NpmTest,
    NpmBuild,
    NpxTest,
    NpxBuild,
    DotnetTest,
    DotnetBuild,
    ClangFormat,
    CmakeConfigure,
    CmakeBuild,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompactedStreams {
    pub(crate) stdout: Option<String>,
    pub(crate) stderr: Option<String>,
}

impl CompactedStreams {
    pub(crate) fn compacted(&self) -> bool {
        self.stdout.is_some() || self.stderr.is_some()
    }
}

pub(crate) fn classify_passthrough(program: &str, args: &[String]) -> Option<OutputProfile> {
    match executable_name(program) {
        "node" if node_test_args(args) => Some(OutputProfile::NodeTest),
        "npm" => classify_npm(args),
        "npx" => classify_npx(args),
        "dotnet" => classify_dotnet(args),
        name if name.starts_with("clang-format") && clang_format_diagnostic_args(args) => {
            Some(OutputProfile::ClangFormat)
        }
        "cmake" => classify_cmake(args),
        _ => None,
    }
}

pub(crate) fn compact_streams(
    profile: OutputProfile,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) -> CompactedStreams {
    let compacted_stdout = compact_stream(profile, stdout, exit_code);
    let compacted_stderr = compact_stream(profile, stderr, exit_code);
    CompactedStreams {
        stdout: compacted_stdout,
        stderr: compacted_stderr,
    }
}

pub(crate) fn summarize_cmake_build(output: &str, exit_code: i32) -> Option<String> {
    let lines = nonempty_lines(output);
    if lines.is_empty() {
        return Some(if exit_code == 0 {
            "cmake build: ok".to_string()
        } else {
            "cmake build: failed".to_string()
        });
    }

    if exit_code != 0 {
        return summarize_cmake_failure(&lines, exit_code);
    }

    let relevant = lines
        .iter()
        .copied()
        .filter(|line| is_build_progress(line) || is_build_summary(line))
        .collect::<Vec<_>>();
    let source = if relevant.is_empty() {
        &lines
    } else {
        &relevant
    };
    let mut result = "cmake build: ok".to_string();
    let window = fallback_window(&source.join("\n"), 8, 18);
    if !window.trim().is_empty() {
        result.push('\n');
        result.push_str(&window);
    }
    Some(result)
}

fn summarize_cmake_failure(lines: &[&str], exit_code: i32) -> Option<String> {
    let selection = cmake_failure::select(lines)?;
    let mut result = render_selection("cmake build", exit_code, lines, &selection.selected);
    if selection.repeated_warning_count > 0 || selection.omitted_unique_warnings > 0 {
        result.push_str("\n[warning compaction:");
        if selection.repeated_warning_count > 0 {
            result.push_str(&format!(
                " {} repeated warning lines suppressed",
                selection.repeated_warning_count
            ));
        }
        if selection.repeated_warning_count > 0 && selection.omitted_unique_warnings > 0 {
            result.push(';');
        }
        if selection.omitted_unique_warnings > 0 {
            result.push_str(&format!(
                " {} unique warning lines omitted",
                selection.omitted_unique_warnings
            ));
        }
        result.push(']');
    }
    Some(result)
}

fn compact_stream(profile: OutputProfile, output: &str, exit_code: i32) -> Option<String> {
    if output.is_empty() || !large_enough_to_compact(output) {
        return None;
    }
    let summary = match profile {
        OutputProfile::NodeTest
        | OutputProfile::NpmTest
        | OutputProfile::NpxTest
        | OutputProfile::DotnetTest => summarize_test(profile, output, exit_code),
        OutputProfile::NpmBuild | OutputProfile::NpxBuild | OutputProfile::DotnetBuild => {
            summarize_build(profile, output, exit_code)
        }
        OutputProfile::ClangFormat => summarize_clang_format(output, exit_code),
        OutputProfile::CmakeConfigure => summarize_cmake_configure(output, exit_code),
        OutputProfile::CmakeBuild => summarize_cmake_build(output, exit_code),
    }?;
    (summary.len() < output.len()).then_some(summary)
}

fn executable_name(program: &str) -> &str {
    let name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".cmd"))
        .unwrap_or(name)
}

fn node_test_args(args: &[String]) -> bool {
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--test" || arg.starts_with("--test=") {
            return true;
        }
        if arg == "--" || arg == "-" || !arg.starts_with('-') || is_node_execution_mode(arg) {
            return false;
        }
        index += if node_option_takes_value(arg) && !arg.contains('=') {
            2
        } else {
            1
        };
    }
    false
}

fn is_node_execution_mode(arg: &str) -> bool {
    matches!(arg, "-c" | "--check" | "-e" | "--eval" | "-p" | "--print")
}

fn node_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-r" | "--require"
            | "--import"
            | "--loader"
            | "--experimental-loader"
            | "--conditions"
            | "--diagnostic-dir"
            | "--env-file"
            | "--icu-data-dir"
            | "--input-type"
            | "--inspect-port"
            | "--openssl-config"
            | "--redirect-warnings"
            | "--test-concurrency"
            | "--test-name-pattern"
            | "--test-reporter"
            | "--test-reporter-destination"
            | "--test-shard"
            | "--title"
            | "--watch-path"
    )
}

fn classify_npm(args: &[String]) -> Option<OutputProfile> {
    let command_index = first_command_index(args)?;
    match args[command_index].as_str() {
        "test" | "t" | "tst" => Some(OutputProfile::NpmTest),
        "run" | "run-script" => {
            let script_args = args.get(command_index + 1..)?;
            let script_index = first_command_index(script_args)?;
            classify_script_name(&script_args[script_index])
        }
        _ => None,
    }
}

fn classify_script_name(script: &str) -> Option<OutputProfile> {
    if script == "test" || script.starts_with("test:") {
        Some(OutputProfile::NpmTest)
    } else if script == "build" || script.starts_with("build:") {
        Some(OutputProfile::NpmBuild)
    } else {
        None
    }
}

fn classify_npx(args: &[String]) -> Option<OutputProfile> {
    let command_index = first_command_index(args)?;
    let command = executable_name(&args[command_index]);
    let command_args = &args[command_index + 1..];
    if matches!(command, "jest" | "vitest" | "mocha" | "ava" | "tap")
        || (command == "node" && node_test_args(command_args))
    {
        return Some(OutputProfile::NpxTest);
    }
    if matches!(
        command,
        "vite" | "webpack" | "rollup" | "parcel" | "esbuild" | "next" | "nuxt" | "astro"
    ) && command_args.iter().any(|arg| arg == "build")
    {
        return Some(OutputProfile::NpxBuild);
    }
    None
}

fn classify_dotnet(args: &[String]) -> Option<OutputProfile> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "build" => return Some(OutputProfile::DotnetBuild),
            "test" => return Some(OutputProfile::DotnetTest),
            "--" => return None,
            _ if !arg.starts_with('-') || arg == "-" => return None,
            _ => {
                index += if dotnet_global_option_takes_value(arg) && !arg.contains('=') {
                    2
                } else {
                    1
                };
            }
        }
    }
    None
}

fn dotnet_global_option_takes_value(arg: &str) -> bool {
    matches!(arg, "--arch" | "--diagnostics" | "--os" | "--roll-forward")
}

fn clang_format_diagnostic_args(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(arg.as_str(), "--dry-run" | "-n" | "--Werror") || arg.starts_with("--Werror=")
    })
}

fn classify_cmake(args: &[String]) -> Option<OutputProfile> {
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-E" | "-P" | "--find-package" | "--install" | "--open" | "--workflow"
        )
    }) {
        return None;
    }
    if args.iter().any(|arg| arg == "--build") {
        return Some(OutputProfile::CmakeBuild);
    }
    let configure_shape = args.iter().any(|arg| {
        matches!(arg.as_str(), "-S" | "-B" | "-G" | "--preset" | "--fresh")
            || arg.starts_with("-S")
            || arg.starts_with("-B")
            || arg.starts_with("--preset=")
    });
    configure_shape.then_some(OutputProfile::CmakeConfigure)
}

fn first_command_index(args: &[String]) -> Option<usize> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return (index + 1 < args.len()).then_some(index + 1);
        }
        if !arg.starts_with('-') || arg == "-" {
            return Some(index);
        }
        if option_takes_value(arg) && !arg.contains('=') {
            index += 2;
        } else {
            index += 1;
        }
    }
    None
}

fn option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "--prefix"
            | "--workspace"
            | "-w"
            | "--userconfig"
            | "--registry"
            | "--cache"
            | "--loglevel"
            | "--package"
            | "-p"
            | "--node-options"
    )
}

fn summarize_test(profile: OutputProfile, output: &str, exit_code: i32) -> Option<String> {
    let lines = output.lines().collect::<Vec<_>>();
    if !recognizes_test_output(profile, &lines) {
        return None;
    }
    let mut selected = vec![false; lines.len()];
    mark_first_nonempty(&lines, &mut selected, 8);
    mark_last_nonempty(&lines, &mut selected, 16);

    let mut result_indices = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if is_test_summary(line) {
            selected[index] = true;
        }
        if is_test_result(line) {
            result_indices.push(index);
        }
        if is_failure_evidence(line) {
            mark_context(&mut selected, index, 2, 12);
        }
    }
    mark_edge_indices(&mut selected, &result_indices, 8, 4);
    Some(render_selection(
        profile.label(),
        exit_code,
        &lines,
        &selected,
    ))
}

fn summarize_build(profile: OutputProfile, output: &str, exit_code: i32) -> Option<String> {
    let lines = output.lines().collect::<Vec<_>>();
    if !recognizes_build_output(profile, &lines) {
        return None;
    }
    let mut selected = vec![false; lines.len()];
    mark_first_nonempty(&lines, &mut selected, 8);
    mark_last_nonempty(&lines, &mut selected, 18);
    for (index, line) in lines.iter().enumerate() {
        if is_build_summary(line) || is_dotnet_summary(line) || is_web_build_summary(line) {
            selected[index] = true;
        }
        if is_build_diagnostic(line) {
            mark_context(&mut selected, index, 1, 4);
        }
    }
    Some(render_selection(
        profile.label(),
        exit_code,
        &lines,
        &selected,
    ))
}

fn summarize_clang_format(output: &str, exit_code: i32) -> Option<String> {
    let lines = output.lines().collect::<Vec<_>>();
    let diagnostics = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| is_clang_format_diagnostic(line).then_some(index))
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        return None;
    }
    let mut selected = vec![false; lines.len()];
    mark_edge_indices(&mut selected, &diagnostics, 50, 20);
    Some(render_selection(
        "clang-format diagnostics",
        exit_code,
        &lines,
        &selected,
    ))
}

fn summarize_cmake_configure(output: &str, exit_code: i32) -> Option<String> {
    let lines = output.lines().collect::<Vec<_>>();
    if !lines.iter().any(|line| is_cmake_configure_line(line)) {
        return None;
    }
    let mut selected = vec![false; lines.len()];
    mark_first_nonempty(&lines, &mut selected, 10);
    mark_last_nonempty(&lines, &mut selected, 18);
    for (index, line) in lines.iter().enumerate() {
        if is_cmake_configure_summary(line) {
            selected[index] = true;
        }
        if is_build_diagnostic(line) {
            mark_context(&mut selected, index, 2, 6);
        }
    }
    Some(render_selection(
        "cmake configure",
        exit_code,
        &lines,
        &selected,
    ))
}

impl OutputProfile {
    fn label(self) -> &'static str {
        match self {
            Self::NodeTest => "node test",
            Self::NpmTest => "npm test",
            Self::NpmBuild => "npm build",
            Self::NpxTest => "npx test",
            Self::NpxBuild => "npx build",
            Self::DotnetTest => "dotnet test",
            Self::DotnetBuild => "dotnet build",
            Self::ClangFormat => "clang-format diagnostics",
            Self::CmakeConfigure => "cmake configure",
            Self::CmakeBuild => "cmake build",
        }
    }
}

fn recognizes_test_output(profile: OutputProfile, lines: &[&str]) -> bool {
    if profile == OutputProfile::NodeTest
        && lines.iter().any(|line| {
            line.contains("TAP version ")
                || line.trim_start().starts_with("# tests ")
                || line.contains("\u{2139} tests ")
        })
    {
        return true;
    }
    let has_result = lines.iter().any(|line| is_test_result(line));
    let has_summary = lines.iter().any(|line| is_test_summary(line));
    has_result && has_summary
}

fn recognizes_build_output(profile: OutputProfile, lines: &[&str]) -> bool {
    match profile {
        OutputProfile::DotnetBuild => lines.iter().any(|line| is_dotnet_summary(line)),
        OutputProfile::NpmBuild => {
            lines.iter().any(|line| is_npm_build_header(line))
                && lines.iter().any(|line| is_web_build_summary(line))
        }
        OutputProfile::NpxBuild => lines.iter().any(|line| is_web_build_summary(line)),
        _ => false,
    }
}

fn large_enough_to_compact(output: &str) -> bool {
    output.len() >= MIN_COMPACT_BYTES || output.lines().count() > MIN_COMPACT_LINES
}

fn render_selection(label: &str, exit_code: i32, lines: &[&str], selected: &[bool]) -> String {
    let selected_indices = selected
        .iter()
        .enumerate()
        .filter_map(|(index, keep)| keep.then_some(index))
        .collect::<Vec<_>>();
    let selected_indices = bounded_indices(&selected_indices);
    let state = if exit_code == 0 {
        "ok".to_string()
    } else {
        format!("failed (exit {exit_code})")
    };
    let mut rendered = format!(
        "{label}: {state} ({} raw lines, {} evidence lines)",
        lines.len(),
        selected_indices.len()
    );
    let mut previous = None;
    for index in selected_indices {
        let omitted = match previous {
            Some(previous) => index.saturating_sub(previous + 1),
            None => index,
        };
        if omitted > 0 {
            rendered.push_str(&format!("\n... [{omitted} lines omitted] ..."));
        }
        rendered.push('\n');
        rendered.push_str(lines[index]);
        previous = Some(index);
    }
    if let Some(previous) = previous {
        let omitted = lines.len().saturating_sub(previous + 1);
        if omitted > 0 {
            rendered.push_str(&format!("\n... [{omitted} lines omitted] ..."));
        }
    }
    rendered
}

fn bounded_indices(indices: &[usize]) -> Vec<usize> {
    if indices.len() <= MAX_EVIDENCE_LINES {
        return indices.to_vec();
    }
    let head = MAX_EVIDENCE_LINES * 2 / 3;
    let tail = MAX_EVIDENCE_LINES - head;
    indices[..head]
        .iter()
        .chain(indices[indices.len() - tail..].iter())
        .copied()
        .collect()
}

fn mark_first_nonempty(lines: &[&str], selected: &mut [bool], count: usize) {
    for (index, _) in lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(count)
    {
        selected[index] = true;
    }
}

fn mark_last_nonempty(lines: &[&str], selected: &mut [bool], count: usize) {
    for (index, _) in lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(count)
    {
        selected[index] = true;
    }
}

fn mark_context(selected: &mut [bool], index: usize, before: usize, after: usize) {
    let start = index.saturating_sub(before);
    let end = (index + after + 1).min(selected.len());
    for keep in &mut selected[start..end] {
        *keep = true;
    }
}

fn mark_edge_indices(selected: &mut [bool], indices: &[usize], head: usize, tail: usize) {
    for index in indices.iter().take(head) {
        selected[*index] = true;
    }
    for index in indices.iter().rev().take(tail) {
        selected[*index] = true;
    }
}

fn nonempty_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn is_test_result(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("ok ")
        || trimmed.starts_with("not ok ")
        || trimmed.starts_with("PASS ")
        || trimmed.starts_with("FAIL ")
        || trimmed.starts_with("[PASS]")
        || trimmed.starts_with("[FAIL]")
        || trimmed.starts_with("Passed ")
        || trimmed.starts_with("Failed ")
        || trimmed.starts_with("\u{2714}")
        || trimmed.starts_with("\u{2716}")
        || trimmed.starts_with("\u{2713}")
        || trimmed.starts_with("\u{2717}")
        || trimmed.starts_with("\u{00d7}")
}

fn is_test_summary(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    trimmed.starts_with("1..")
        || lower.starts_with("# tests ")
        || lower.starts_with("# suites ")
        || lower.starts_with("# pass ")
        || lower.starts_with("# fail ")
        || lower.starts_with("# skipped ")
        || lower.starts_with("# todo ")
        || lower.starts_with("# duration_ms ")
        || lower.contains("test suites:")
        || lower.starts_with("tests:")
        || lower.starts_with("test result:")
        || lower.contains("tests passed")
        || lower.contains("tests failed")
        || is_unpunctuated_test_summary(&lower)
        || lower.starts_with("passed!")
        || lower.starts_with("failed!")
        || trimmed.starts_with("\u{2139} tests ")
        || trimmed.starts_with("\u{2139} pass ")
        || trimmed.starts_with("\u{2139} fail ")
        || trimmed.starts_with("\u{2139} duration_ms ")
}

fn is_failure_evidence(line: &str) -> bool {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    trimmed.starts_with("not ok ")
        || trimmed.starts_with("FAIL ")
        || trimmed.starts_with("[FAIL]")
        || trimmed.starts_with("Failed ")
        || trimmed.starts_with("\u{2716}")
        || trimmed.starts_with("\u{2717}")
        || trimmed.starts_with("\u{00d7}")
        || lower.contains("assertionerror")
        || lower.contains("panic")
        || lower.contains("error:")
        || lower.contains("failed test")
        || lower.contains("failing test")
        || lower.starts_with("expected ")
        || lower.starts_with("actual ")
}

fn is_unpunctuated_test_summary(lower: &str) -> bool {
    let status = lower.contains(" passed")
        || lower.contains(" failed")
        || lower.contains(" skipped")
        || lower.contains(" passing")
        || lower.contains(" failing");
    if !status {
        return false;
    }
    lower.starts_with("tests ")
        || lower.starts_with("test files ")
        || lower
            .split_whitespace()
            .next()
            .is_some_and(|value| value.parse::<usize>().is_ok())
}

fn is_npm_build_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('>') && trimmed.to_ascii_lowercase().contains(" build")
}

fn is_web_build_summary(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower.contains("built in ")
        || lower.contains("compiled successfully")
        || lower.contains("compiled with ")
        || lower.contains("modules transformed")
        || lower.contains("build completed")
        || lower.contains("build succeeded")
        || lower.contains("build failed")
        || lower.starts_with("webpack ")
        || lower.starts_with("vite v")
}

fn is_dotnet_summary(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower == "build succeeded."
        || lower == "build failed."
        || lower.ends_with(" warning(s)")
        || lower.ends_with(" error(s)")
        || lower.starts_with("passed!")
        || lower.starts_with("failed!")
        || lower.starts_with("total tests:")
}

fn is_clang_format_diagnostic(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("code should be clang-formatted")
        || lower.contains("wclang-format-violations")
        || lower.contains("clang-format")
            && (lower.contains("warning:") || lower.contains("error:"))
}

fn is_cmake_configure_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("-- The ")
        || trimmed.starts_with("-- Detecting ")
        || trimmed.starts_with("-- Check for working ")
        || is_cmake_configure_summary(trimmed)
        || trimmed.starts_with("CMake Error")
        || trimmed.starts_with("CMake Warning")
}

fn is_cmake_configure_summary(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("-- Configuring done")
        || trimmed.starts_with("-- Generating done")
        || trimmed.starts_with("-- Build files have been written to:")
}

fn is_build_diagnostic(line: &str) -> bool {
    cmake_failure::is_diagnostic(line)
}

fn is_build_progress(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('[')
        || trimmed.starts_with("Building ")
        || trimmed.starts_with("Linking ")
        || trimmed.starts_with("Scanning ")
        || trimmed.starts_with("Generating ")
}

fn is_build_summary(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("built target")
        || lower.contains("no work to do")
        || lower.contains("up to date")
        || lower.contains("build files have been written")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn long_tap(failed: bool) -> String {
        let mut output = String::from("TAP version 13\n");
        for index in 1..=100 {
            if failed && index == 57 {
                output.push_str("not ok 57 - keeps the failing assertion\n");
                output.push_str("  error: expected true but received false\n");
            } else {
                output.push_str(&format!("ok {index} - test {index}\n"));
            }
        }
        output.push_str("1..100\n# tests 100\n# pass 99\n# fail 1\n# duration_ms 40\n");
        output
    }

    #[test]
    fn classifier_limits_compaction_to_high_confidence_shapes() {
        assert_eq!(
            classify_passthrough("node", &strings(&["--test", "suite.mjs"])),
            Some(OutputProfile::NodeTest)
        );
        assert_eq!(
            classify_passthrough("npm", &strings(&["--prefix", "web", "run", "build:web"])),
            Some(OutputProfile::NpmBuild)
        );
        assert_eq!(
            classify_passthrough("npx", &strings(&["vitest", "run"])),
            Some(OutputProfile::NpxTest)
        );
        assert_eq!(
            classify_passthrough("dotnet", &strings(&["test", "service.csproj"])),
            Some(OutputProfile::DotnetTest)
        );
        assert_eq!(
            classify_passthrough("clang-format-18", &strings(&["--dry-run", "src/a.cpp"])),
            Some(OutputProfile::ClangFormat)
        );
        assert_eq!(
            classify_passthrough("/opt/bin/cmake", &strings(&["-S", ".", "-B", "build"])),
            Some(OutputProfile::CmakeConfigure)
        );

        assert_eq!(
            classify_passthrough("node", &strings(&["script.mjs"])),
            None
        );
        assert_eq!(
            classify_passthrough("node", &strings(&["script.mjs", "--test"])),
            None
        );
        assert_eq!(
            classify_passthrough("node", &strings(&["-e", "console.log('test')", "--test"])),
            None
        );
        assert_eq!(
            classify_passthrough(
                "node",
                &strings(&["--test-reporter", "spec", "--test", "suite.mjs"])
            ),
            Some(OutputProfile::NodeTest)
        );
        assert_eq!(
            classify_passthrough("jq", &strings(&[".", "data.json"])),
            None
        );
        assert_eq!(
            classify_passthrough("curl", &strings(&["https://example.test"])),
            None
        );
        assert_eq!(
            classify_passthrough("bash", &strings(&["-lc", "make test"])),
            None
        );
        assert_eq!(
            classify_passthrough("npm", &strings(&["run", "lint"])),
            None
        );
        assert_eq!(
            classify_passthrough("npx", &strings(&["prettier", "--check", "."])),
            None
        );
        assert_eq!(
            classify_passthrough("npx", &strings(&["custom-tool", "test"])),
            None
        );
        assert_eq!(
            classify_passthrough("npx", &strings(&["custom-tool", "build"])),
            None
        );
        assert_eq!(
            classify_passthrough("dotnet", &strings(&["run", "service.csproj"])),
            None
        );
        assert_eq!(
            classify_passthrough("dotnet", &strings(&["run", "--project", "test"])),
            None
        );
        assert_eq!(
            classify_passthrough("dotnet", &strings(&["--arch", "x64", "test"])),
            Some(OutputProfile::DotnetTest)
        );
        assert_eq!(
            classify_passthrough("clang-format", &strings(&["-i", "src/a.cpp"])),
            None
        );
        assert_eq!(
            classify_passthrough("cmake", &strings(&["-P", "script.cmake"])),
            None
        );
        assert_eq!(
            classify_passthrough("cmake", &strings(&["-P", "script.cmake", "--build"])),
            None
        );
    }

    #[test]
    fn node_tap_summary_retains_counters_and_failure_evidence() {
        let raw = long_tap(true);
        let compacted = compact_streams(OutputProfile::NodeTest, &raw, "", 1);
        assert!(compacted.compacted());
        let stdout = compacted.stdout.as_deref().unwrap();
        assert!(stdout.contains("node test: failed (exit 1)"));
        assert!(stdout.contains("not ok 57"));
        assert!(stdout.contains("expected true but received false"));
        assert!(stdout.contains("# tests 100"));
        assert!(stdout.contains("# fail 1"));
        assert!(stdout.contains("lines omitted"));
        assert!(stdout.len() < raw.len());
    }

    #[test]
    fn unrecognized_test_text_remains_exact() {
        let raw = (1..=100)
            .map(|index| format!("application line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let compacted = compact_streams(OutputProfile::NpmTest, &raw, "", 0);
        assert!(!compacted.compacted());
        assert!(compacted.stdout.is_none());
        assert!(compacted.stderr.is_none());
    }

    #[test]
    fn dotnet_build_summary_keeps_result_and_diagnostics() {
        let mut raw = (1..=100)
            .map(|index| format!("  Restored project {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        raw.push_str("\nsrc/App.cs(10,2): error CS1002: ; expected\nBuild FAILED.\n0 Warning(s)\n1 Error(s)\n");
        let compacted = compact_streams(OutputProfile::DotnetBuild, &raw, "", 1);
        assert!(compacted.compacted());
        let stdout = compacted.stdout.as_deref().unwrap();
        assert!(stdout.contains("dotnet build: failed (exit 1)"));
        assert!(stdout.contains("error CS1002"));
        assert!(stdout.contains("Build FAILED."));
        assert!(stdout.contains("1 Error(s)"));
    }

    #[test]
    fn clang_format_summary_is_diagnostic_only() {
        let raw = (1..=100)
            .map(|index| {
                format!(
                    "src/file{index}.cpp:1:1: warning: code should be clang-formatted [-Wclang-format-violations]"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let compacted = compact_streams(OutputProfile::ClangFormat, &raw, "", 1);
        assert!(compacted.compacted());
        let stdout = compacted.stdout.as_deref().unwrap();
        assert!(stdout.contains("clang-format diagnostics: failed (exit 1)"));
        assert!(stdout.contains("file1.cpp"));
        assert!(stdout.contains("file100.cpp"));
    }

    #[test]
    fn cmake_configure_summary_preserves_terminal_authority() {
        let mut raw = String::from("-- The CXX compiler identification is AppleClang 18\n");
        for index in 1..=100 {
            raw.push_str(&format!("-- Detecting CXX compile feature {index}\n"));
        }
        raw.push_str(
            "-- Configuring done\n-- Generating done\n-- Build files have been written to: /work/build\n",
        );
        let compacted = compact_streams(OutputProfile::CmakeConfigure, &raw, "", 0);
        assert!(compacted.compacted());
        let stdout = compacted.stdout.as_deref().unwrap();
        assert!(stdout.contains("cmake configure: ok"));
        assert!(stdout.contains("-- Configuring done"));
        assert!(stdout.contains("-- Build files have been written to: /work/build"));
    }
}
