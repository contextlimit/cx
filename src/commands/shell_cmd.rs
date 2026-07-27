use std::path::Path;
use std::process::Command;

use anyhow::Result;

mod analysis;

use crate::support::insights::{self, CommandOpportunityRecord, TextMetrics};
use crate::support::output_projection::GENERATED_LINE_PREVIEW_CHARS;
use crate::support::output_window::{OutputWindow, ProjectedOutput};
use crate::support::redaction;
use crate::support::runner::{
    append_failure_hint, capture_with_piped_stdin, CommandOutput, ProxyOutcome,
};
use crate::support::{command_repair, shell_hints, utils::resolve_binary};
use analysis::{
    literal_newline_script_error, shell_command_arg, should_preserve_exact_stdout,
    split_shell_statements, split_shell_words,
};

const SHELL_OUTPUT_WINDOW: OutputWindow = OutputWindow::new(12, 28);
const SMALL_EXACT_OUTPUT_LINES: usize = 110;
const SMALL_EXACT_OUTPUT_BYTES: usize = 64 * 1024;
const LINE_WINDOW_STRATEGY: &str = "shell-head-tail-12-28";
const GENERATED_LINE_STRATEGY: &str = "shell-generated-line-1200";
const COMBINED_STRATEGY: &str = "shell-head-tail-12-28-generated-line-1200";

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    run_with_options(args, false)
}

pub fn run_with_options(args: &[String], no_compact: bool) -> Result<ProxyOutcome> {
    if let Some(message) = literal_newline_script_error(args) {
        return Ok(ProxyOutcome {
            stdout: String::new(),
            stderr: message,
            exit_code: 2,
            observation: None,
        });
    }
    let mut cmd = resolved_bash_command();
    if args.is_empty() {
        cmd.arg("-s");
    } else {
        cmd.args(args);
    }
    let mut output = capture_shell(cmd)?;
    let raw_output = output.combined.clone();
    let optimizations_enabled =
        insights::command_optimizations_enabled().unwrap_or(true) && !no_compact;
    let preserve_exact_stdout =
        should_preserve_exact_stdout(args) || should_preserve_small_exact_stdout(&output.stdout);
    if optimizations_enabled && !preserve_exact_stdout {
        record_shell_opportunity(args, &output);
    }
    let stdout_optimizations_enabled = optimizations_enabled && !preserve_exact_stdout;
    let mut stdout = shell_stdout(&output, stdout_optimizations_enabled);
    let mut stderr = shell_stderr(&output, optimizations_enabled);
    if output.exit_code != 0 {
        let failure_hint = output.failure_artifact_hint("sh");
        stdout = append_failure_hint(
            shell_stdout(&output, stdout_optimizations_enabled),
            failure_hint.as_deref(),
        );
        shell_hints::append_hint(&mut stderr, &raw_output);
        append_shell_repair_advice(&mut stderr, args, output.exit_code, &raw_output);
    }
    let observation = output.observation("sh");
    Ok(ProxyOutcome {
        stdout,
        stderr,
        exit_code: output.exit_code,
        observation: None,
    }
    .with_observation(observation))
}

fn append_shell_repair_advice(stderr: &mut String, args: &[String], exit_code: i32, output: &str) {
    let Some(script) = shell_command_arg(args) else {
        return;
    };
    let Some(advice) = shell_repair_advice(script, exit_code, output) else {
        return;
    };
    command_repair::append_note(stderr, &advice.note);
    record_shell_repair_advice(args, exit_code, output, &advice);
}

fn shell_repair_advice(
    script: &str,
    exit_code: i32,
    output: &str,
) -> Option<command_repair::CommandAdvice> {
    let statements = split_shell_statements(script);
    if statements.len() != 1 {
        return None;
    }
    let words = split_shell_words(statements[0])?;
    let program = words.first()?.as_str();
    command_repair::shell_command_advice(program, &words[1..], exit_code, output)
}

fn record_shell_repair_advice(
    args: &[String],
    exit_code: i32,
    output: &str,
    advice: &command_repair::CommandAdvice,
) {
    let mut command_args = Vec::with_capacity(args.len() + 1);
    command_args.push("sh".to_string());
    command_args.extend(args.iter().cloned());
    let command = redaction::redacted_shell_join(&command_args);
    let record = insights::CommandRepairRecord {
        process: "sh",
        command_family: "sh",
        command: &command,
        source: "sh",
        rule_id: advice.rule_id,
        action: "advice",
        original_exit_code: exit_code,
        final_exit_code: exit_code,
        original_response: output,
        final_response: output,
    };
    let _ = insights::record_command_repair(&record);
}

fn resolved_bash_command() -> Command {
    if let Ok(path) = resolve_binary("bash") {
        return Command::new(path);
    }
    for candidate in ["/bin/bash", "/usr/bin/bash"] {
        if Path::new(candidate).is_file() {
            return Command::new(candidate);
        }
    }
    Command::new("bash")
}

fn capture_shell(cmd: std::process::Command) -> Result<CommandOutput> {
    capture_with_piped_stdin(cmd, "sh")
}

fn shell_stdout(output: &CommandOutput, optimizations_enabled: bool) -> String {
    if optimizations_enabled {
        bounded_stdout(&output.stdout)
    } else {
        output.stdout.clone()
    }
}

fn shell_stderr(output: &CommandOutput, optimizations_enabled: bool) -> String {
    if optimizations_enabled {
        bounded_stderr(&output.stderr)
    } else {
        output.stderr.trim_end().to_string()
    }
}

fn bounded_stdout(stdout: &str) -> String {
    SHELL_OUTPUT_WINDOW
        .project(stdout, GENERATED_LINE_PREVIEW_CHARS)
        .text
}

fn bounded_stderr(stderr: &str) -> String {
    bounded_stdout(stderr).trim_end().to_string()
}

fn should_preserve_small_exact_stdout(stdout: &str) -> bool {
    if stdout.len() > SMALL_EXACT_OUTPUT_BYTES || stdout.lines().count() > SMALL_EXACT_OUTPUT_LINES
    {
        return false;
    }
    !SHELL_OUTPUT_WINDOW
        .project(stdout, GENERATED_LINE_PREVIEW_CHARS)
        .generated_lines_bounded
}

fn record_shell_opportunity(args: &[String], output: &CommandOutput) {
    let projection = SHELL_OUTPUT_WINDOW.project(&output.combined, GENERATED_LINE_PREVIEW_CHARS);
    if projection.text == output.combined {
        return;
    }
    let mut command_args = Vec::with_capacity(args.len() + 1);
    command_args.push("sh".to_string());
    command_args.extend(args.iter().cloned());
    let command = redaction::redacted_shell_join(&command_args);
    let record = CommandOpportunityRecord {
        process: "sh",
        command_family: "sh",
        command: &command,
        source: "sh",
        strategy: projection_strategy(&projection),
        confidence: projection_confidence(&projection),
        raw: output.observation("sh").metrics,
        projected: TextMetrics::from_text(&projection.text),
    };
    let _ = insights::record_command_opportunity(&record);
}

fn projection_confidence(projection: &ProjectedOutput) -> insights::OpportunityConfidence {
    match (projection.line_windowed, projection.generated_lines_bounded) {
        (false, true) => insights::OpportunityConfidence::High,
        (true, true) => insights::OpportunityConfidence::Medium,
        _ => insights::OpportunityConfidence::Low,
    }
}

fn projection_strategy(projection: &ProjectedOutput) -> &'static str {
    match (projection.line_windowed, projection.generated_lines_bounded) {
        (true, true) => COMBINED_STRATEGY,
        (false, true) => GENERATED_LINE_STRATEGY,
        _ => LINE_WINDOW_STRATEGY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::test_support::with_env_vars;

    fn run_without_insights(args: &[&str]) -> ProxyOutcome {
        let home = tempfile::tempdir().expect("temporary shell test home");
        let home_string = home.path().to_string_lossy().to_string();
        let values = args
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        with_env_vars(
            &[
                ("CX_DISABLE_INSIGHTS", Some("1")),
                ("HOME", Some(home_string.as_str())),
            ],
            || run(&values).expect("shell command should run"),
        )
    }

    #[test]
    fn short_stdout_stays_unwindowed() {
        let outcome = run_without_insights(&["-lc", "printf 'alpha\\nbeta\\n'"]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout, "alpha\nbeta\n");
        assert!(outcome.stderr.is_empty());
    }

    #[test]
    fn long_stdout_is_windowed_and_keeps_evidence() {
        let outcome = run_without_insights(&[
            "-lc",
            "for i in {1..160}; do printf 'line_%03d\\n' \"$i\"; done",
        ]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("line_001"));
        assert!(outcome.stdout.contains("line_160"));
        assert!(outcome.stdout.contains("... [120 lines omitted] ..."));
        assert!(!outcome.stdout.contains("line_080"));
        assert!(outcome
            .observation
            .as_ref()
            .is_some_and(|raw| raw.metrics.lines == 160));
    }

    #[test]
    fn modest_stdout_stays_exact_without_static_script_analysis() {
        let outcome = run_without_insights(&[
            "-lc",
            "for i in {1..103}; do printf 'bounded_%03d\\n' \"$i\"; done",
        ]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 103);
        assert!(outcome.stdout.contains("bounded_001"));
        assert!(outcome.stdout.contains("bounded_052"));
        assert!(outcome.stdout.contains("bounded_103"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn no_compact_preserves_long_stdout_exactly() {
        let values = [
            "-lc",
            "for i in {1..80}; do printf 'line_%02d\\n' \"$i\"; done",
        ]
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
        let outcome = with_env_vars(&[("CX_DISABLE_INSIGHTS", Some("1"))], || {
            run_with_options(&values, true).expect("shell command should run")
        });

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 80);
        assert!(outcome.stdout.contains("line_01"));
        assert!(outcome.stdout.contains("line_80"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn generated_one_line_json_is_bounded_and_keeps_both_ends() {
        let raw = format!(
            r#"{{"status":"ok","payload":"{}","tail":"complete"}}"#,
            "A".repeat(4_000)
        );
        let script = format!("printf '%s\\n' {}", shell_quote(&raw));
        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.starts_with(r#"{"status":"ok""#));
        assert!(outcome.stdout.ends_with("\"tail\":\"complete\"}\n"));
        assert!(outcome.stdout.contains("generated line truncated"));
        assert!(outcome.stdout.len() < raw.len() / 2);
        assert!(outcome
            .observation
            .as_ref()
            .is_some_and(|observation| observation.metrics.chars > 4_000));
    }

    #[test]
    fn long_source_and_css_lines_are_not_treated_as_generated_output() {
        let source = format!(
            "const matcher = /{}/; return matcher.test(candidate);",
            "route|path|dashboard|projects|migration|brain|agents|skills|".repeat(40)
        );
        let css = format!(
            ".panel{{--panel-copy:'{}';color:var(--panel-text,#fff);display:grid;}}",
            "human readable css value ".repeat(90)
        );
        for raw in [source, css] {
            let script = format!("printf '%s\\n' {}", shell_quote(&raw));
            let outcome = run_without_insights(&["-lc", &script]);

            assert_eq!(outcome.exit_code, 0);
            assert_eq!(outcome.stdout, format!("{raw}\n"));
            assert!(!outcome.stdout.contains("generated line truncated"));
        }
    }

    #[test]
    fn numbered_sed_range_pipelines_stay_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lines.txt");
        let content = (1..=900)
            .map(|line| format!("line_{line:03}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!(
            "nl -ba {quoted} | sed -n '1,30p'; nl -ba {quoted} | sed -n \"450,485p\"; nl -ba {quoted} | sed -n '708,728p'"
        );

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 87);
        assert!(outcome.stdout.contains("line_001"));
        assert!(outcome.stdout.contains("line_450"));
        assert!(outcome.stdout.contains("line_728"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn numbered_sed_multi_range_pipeline_stays_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("spec.md");
        let content = (1..=900)
            .map(|line| format!("spec_line_{line:03}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!(
            "nl -ba {quoted} | sed -n \"44,56p;180,214p;336,374p;472,492p;542,566p;838,846p\""
        );

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 142);
        assert!(outcome.stdout.contains("spec_line_044"));
        assert!(outcome.stdout.contains("spec_line_214"));
        assert!(outcome.stdout.contains("spec_line_846"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn document_cat_shell_script_stays_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("operator-guide.md");
        let content = (1..=120)
            .map(|line| format!("guide_line_{line:03}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("cat {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 120);
        assert!(outcome.stdout.contains("guide_line_060"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn document_sed_shell_script_stays_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("operator-guide.rst");
        let content = (1..=260)
            .map(|line| format!("guide_line_{line:03}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("sed -n '1,120p' {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 120);
        assert!(outcome.stdout.contains("guide_line_080"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn plan_json_shell_read_stays_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let plan_dir = temp.path().join(".state").join("plans/example");
        std::fs::create_dir_all(&plan_dir).unwrap();
        let path = plan_dir.join("planSteps.json");
        let entries = (1..=100)
            .map(|ordinal| format!(r#"  {{"ordinal":{ordinal},"status":"pending"}}"#))
            .collect::<Vec<_>>()
            .join(",\n");
        let content = format!("[\n{entries}\n]\n");
        std::fs::write(&path, &content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("sed -n '1,$p' {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout, content);
        assert!(outcome.stdout.contains(r#""ordinal":50"#));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn bounded_diff_dd_shell_read_stays_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exact-evidence-r2209.diff");
        let content = (1..=1_800)
            .map(|line| format!("+diff_evidence_{line:04}\n"))
            .collect::<String>();
        std::fs::write(&path, &content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!(
            "dd if={quoted} bs=1 skip=0 count={} 2>/dev/null",
            content.len()
        );

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout, content);
        assert!(outcome.stdout.contains("diff_evidence_0900"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn oversized_diff_dd_request_still_windows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("oversized.diff");
        let content = (1..=120)
            .map(|line| format!("+diff_evidence_{line:04}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!(
            "dd if={quoted} bs=1 skip=0 count={} 2>/dev/null",
            1024 * 1024 + 1
        );

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("diff_evidence_0001"));
        assert!(outcome.stdout.contains("diff_evidence_0120"));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn source_cat_shell_script_still_windows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("main.rs");
        let content = (1..=120)
            .map(|line| format!("fn function_{line:03}() {{}}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("cat {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("function_001"));
        assert!(outcome.stdout.contains("function_120"));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn pipeline_head_limit_stays_unwindowed() {
        let outcome = run_without_insights(&["-lc", "printf 'hit_%03d\\n' {1..300} | head -n 220"]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 220);
        assert!(outcome.stdout.contains("hit_001"));
        assert!(outcome.stdout.contains("hit_220"));
        assert!(outcome.stdout.contains("hit_080"));
        assert!(!outcome.stdout.contains("hit_221"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn direct_source_sed_range_stays_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("main.rs");
        let content = (1..=220)
            .map(|line| format!("fn function_{line:03}() {{}}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("sed -n '1,120p' {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 120);
        assert!(outcome.stdout.contains("function_080"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn large_explicit_source_sed_ranges_stay_unwindowed() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.cpp");
        let second = temp.path().join("second.cpp");
        let first_content = (1..=300)
            .map(|line| format!("int first_{line:04}();\n"))
            .collect::<String>();
        let second_content = (1..=2_600)
            .map(|line| format!("int second_{line:04}();\n"))
            .collect::<String>();
        std::fs::write(&first, first_content).unwrap();
        std::fs::write(&second, second_content).unwrap();
        let first_quoted = shell_quote(first.to_string_lossy().as_ref());
        let second_quoted = shell_quote(second.to_string_lossy().as_ref());
        let script = format!("sed -n '1,260p' {first_quoted}; sed -n '1,2500p' {second_quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.lines().count(), 2_760);
        assert!(outcome.stdout.contains("first_0260"));
        assert!(outcome.stdout.contains("second_1425"));
        assert!(outcome.stdout.contains("second_2500"));
        assert!(!outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn large_generated_source_sed_range_still_windows() {
        let temp = tempfile::tempdir().unwrap();
        let generated = temp.path().join("generated");
        std::fs::create_dir_all(&generated).unwrap();
        let path = generated.join("bindings.cpp");
        let content = (1..=2_600)
            .map(|line| format!("int generated_{line:04}();\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("sed -n '1,2500p' {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("generated_0001"));
        assert!(outcome.stdout.contains("generated_2500"));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn large_json_sed_range_still_windows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("payload.json");
        let content = (1..=2_600)
            .map(|line| format!("{{\"line\":{line}}}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("sed -n '1,2500p' {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains(r#"{"line":1}"#));
        assert!(outcome.stdout.contains(r#"{"line":2500}"#));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn tail_from_start_still_windows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("main.rs");
        let content = (1..=120)
            .map(|line| format!("fn function_{line:03}() {{}}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!("tail -n +1 {quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("function_001"));
        assert!(outcome.stdout.contains("function_120"));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn oversized_head_limit_still_windows() {
        let outcome =
            run_without_insights(&["-lc", "printf 'hit_%03d\\n' {1..120} | head -n 2000"]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("hit_001"));
        assert!(outcome.stdout.contains("hit_120"));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn direct_multi_file_head_still_windows() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.rs");
        let second = temp.path().join("second.rs");
        let first_content = (1..=80)
            .map(|line| format!("fn first_{line:03}() {{}}\n"))
            .collect::<String>();
        let second_content = (1..=80)
            .map(|line| format!("fn second_{line:03}() {{}}\n"))
            .collect::<String>();
        std::fs::write(&first, first_content).unwrap();
        std::fs::write(&second, second_content).unwrap();
        let first_quoted = shell_quote(first.to_string_lossy().as_ref());
        let second_quoted = shell_quote(second.to_string_lossy().as_ref());
        let script = format!("head -n 80 {first_quoted} {second_quoted}");

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("first_001"));
        assert!(outcome.stdout.contains("second_080"));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn mixed_shell_scripts_still_window_output() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lines.txt");
        let content = (1..=80)
            .map(|line| format!("line_{line:03}\n"))
            .collect::<String>();
        std::fs::write(&path, content).unwrap();
        let quoted = shell_quote(path.to_string_lossy().as_ref());
        let script = format!(
            "nl -ba {quoted} | sed -n '1,20p'; for i in {{1..120}}; do printf 'extra_%03d\\n' \"$i\"; done"
        );

        let outcome = run_without_insights(&["-lc", &script]);

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("extra_120"));
        assert!(outcome.stdout.contains("lines omitted"));
    }

    #[test]
    fn bounded_shell_output_is_not_recorded_as_opportunity() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let values = vec![
            "-lc".to_string(),
            "printf 'hit_%03d\\n' {1..300} | head -n 220".to_string(),
        ];
        let db_path_string = db_path.to_string_lossy().to_string();

        let opportunities = with_env_vars(
            &[
                ("CX_DISABLE_INSIGHTS", None),
                ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ],
            || {
                run(&values).expect("shell command should run");
                insights::command_opportunities(5).expect("opportunities should load")
            },
        );

        assert!(opportunities.is_empty());
    }

    #[test]
    fn exact_source_anchor_shell_output_is_not_recorded_as_opportunity() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let lines_path = temp.path().join("lines.txt");
        let content = (1..=900)
            .map(|line| format!("line_{line:03}\n"))
            .collect::<String>();
        std::fs::write(&lines_path, content).unwrap();
        let quoted = shell_quote(lines_path.to_string_lossy().as_ref());
        let script = format!(
            "nl -ba {quoted} | sed -n '1,30p'; nl -ba {quoted} | sed -n '450,485p'; nl -ba {quoted} | sed -n '708,728p'"
        );
        let values = vec!["-lc".to_string(), script];
        let db_path_string = db_path.to_string_lossy().to_string();

        let opportunities = with_env_vars(
            &[
                ("CX_DISABLE_INSIGHTS", None),
                ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ],
            || {
                run(&values).expect("shell command should run");
                insights::command_opportunities(5).expect("opportunities should load")
            },
        );

        assert!(opportunities.is_empty());
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    #[test]
    fn failure_output_is_windowed_and_keeps_artifact_hint() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let home_string = home.to_string_lossy().to_string();
        let args = [
            "-lc",
            "for i in {1..80}; do printf 'out_%02d\\n' \"$i\"; done; for i in {1..80}; do printf 'err_%02d\\n' \"$i\" >&2; done; exit 7",
        ];
        let values = args
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();

        let outcome = with_env_vars(
            &[
                ("CX_DISABLE_INSIGHTS", Some("1")),
                ("HOME", Some(home_string.as_str())),
            ],
            || run(&values).expect("shell command should run"),
        );

        assert_eq!(outcome.exit_code, 7);
        assert!(outcome.stdout.contains("out_01"));
        assert!(outcome.stdout.contains("out_80"));
        assert!(outcome.stdout.contains("[full output:"));
        assert!(outcome.stderr.contains("err_01"));
        assert!(outcome.stderr.contains("err_80"));
        assert!(outcome.stderr.contains("... [40 lines omitted] ..."));
    }

    #[test]
    fn shell_opportunity_records_projected_savings() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let db_path_string = db_path.to_string_lossy().to_string();
        let home_string = temp.path().to_string_lossy().to_string();
        let args = [
            "-lc",
            "for i in {1..160}; do printf 'line_%03d\\n' \"$i\"; done",
        ];
        let values = args
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();

        let opportunities = with_env_vars(
            &[
                ("CX_DISABLE_INSIGHTS", None),
                ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
                ("HOME", Some(home_string.as_str())),
            ],
            || {
                run(&values).expect("shell command should run");
                insights::command_opportunities(5).expect("opportunities should load")
            },
        );

        let shell = opportunities
            .iter()
            .find(|opportunity| opportunity.process == "sh")
            .expect("shell opportunity should be recorded");
        assert_eq!(shell.command_family, "sh");
        assert_eq!(shell.samples, 1);
        assert_eq!(shell.raw.lines, 160);
        assert_eq!(shell.projected.lines, 41);
        assert!(shell.potential_saved.lines > 0);
    }

    #[test]
    fn shell_opportunity_records_generated_one_line_savings() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let db_path_string = db_path.to_string_lossy().to_string();
        let home_string = temp.path().to_string_lossy().to_string();
        let raw = format!(r#"{{"payload":"{}","tail":"done"}}"#, "J".repeat(8_000));
        let script = format!("printf '%s\\n' {}", shell_quote(&raw));
        let values = vec!["-lc".to_string(), script];

        let opportunities = with_env_vars(
            &[
                ("CX_DISABLE_INSIGHTS", None),
                ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
                ("HOME", Some(home_string.as_str())),
            ],
            || {
                run(&values).expect("shell command should run");
                insights::command_opportunities(5).expect("opportunities should load")
            },
        );

        let shell = opportunities
            .iter()
            .find(|opportunity| opportunity.process == "sh")
            .expect("shell opportunity should be recorded");
        assert_eq!(shell.strategy, GENERATED_LINE_STRATEGY);
        assert_eq!(shell.raw.lines, 1);
        assert!(shell.projected.chars <= GENERATED_LINE_PREVIEW_CHARS as u64 + 1);
        assert!(shell.potential_saved.chars > 6_000);
    }

    #[test]
    fn literal_escaped_newline_heredoc_is_rejected_before_bash() {
        let outcome = run_without_insights(&["-lc", "python3 - <<'PY'\\nprint('alpha')\\nPY"]);

        assert_eq!(outcome.exit_code, 2);
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.contains("literal \\n escapes"));
        assert!(outcome.stderr.contains("cx sh <<'BASH'"));
        assert!(!outcome.stderr.contains("syntax error"));
    }

    #[test]
    fn serialized_multiline_script_is_rejected_before_bash() {
        let outcome = run_without_insights(&[
            "-lc",
            "\\nfor item in alpha beta; do\\n  printf '%s\\n' \"$item\"\\ndone\\n",
        ]);

        assert_eq!(outcome.exit_code, 2);
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.contains("serialized \\n separators"));
        assert!(outcome
            .stderr
            .contains("may already have expanded variables"));
        assert!(outcome.stderr.contains("cx sh <<'BASH'"));
        assert!(!outcome.stderr.contains("command not found"));
    }

    #[test]
    fn real_newline_heredoc_still_runs() {
        let outcome = run_without_insights(&["-lc", "python3 - <<'PY'\nprint('alpha')\nPY"]);

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout, "alpha\n");
        assert!(outcome.stderr.is_empty());
    }

    #[test]
    fn command_optimizations_setting_can_disable_shell_windowing() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let db_path_string = db_path.to_string_lossy().to_string();
        let args = [
            "-lc",
            "for i in {1..80}; do printf 'line_%02d\\n' \"$i\"; done",
        ];
        let values = args
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();

        let (outcome, opportunities) = with_env_vars(
            &[
                ("CX_DISABLE_INSIGHTS", None),
                ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ],
            || {
                insights::set_insight_setting("command_optimizations", "false")
                    .expect("setting should save");
                let outcome = run(&values).expect("shell command should run");
                let opportunities =
                    insights::command_opportunities(5).expect("opportunities should load");
                (outcome, opportunities)
            },
        );

        assert_eq!(outcome.stdout.lines().count(), 80);
        assert!(!outcome.stdout.contains("lines omitted"));
        assert!(opportunities.is_empty());
    }
}
