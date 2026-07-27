use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::commands::{passthrough, read};
use crate::support::runner::ProxyOutcome;

pub fn run_cat(args: &[String]) -> Result<ProxyOutcome> {
    if let Some((path, line_numbers)) = parse_cat(args) {
        return run_read(
            path,
            read::ReadOptions {
                line_numbers,
                ..read::ReadOptions::default()
            },
        );
    }
    run_passthrough("cat", args)
}

pub fn run_head(args: &[String]) -> Result<ProxyOutcome> {
    if let Some((path, count)) = parse_head(args) {
        return run_read(
            path,
            read::ReadOptions {
                line_range: Some(read::ReadRange {
                    start: Some(1),
                    end: Some(count),
                }),
                auto_aggressive: false,
                ..read::ReadOptions::default()
            },
        );
    }
    run_passthrough("head", args)
}

pub fn run_tail(args: &[String]) -> Result<ProxyOutcome> {
    if let Some((path, tail_mode)) = parse_tail(args) {
        let line_range = match tail_mode {
            TailMode::Last(count) => tail_last_range(path, count)?,
            TailMode::From(start) => read::ReadRange {
                start: Some(start),
                end: None,
            },
        };
        return run_read(
            path,
            read::ReadOptions {
                line_range: Some(line_range),
                auto_aggressive: false,
                ..read::ReadOptions::default()
            },
        );
    }
    run_passthrough("tail", args)
}

pub fn run_sed(args: &[String]) -> Result<ProxyOutcome> {
    if let Some((path, line_range)) = parse_sed(args) {
        return run_read(
            path,
            read::ReadOptions {
                line_range: Some(line_range),
                auto_aggressive: false,
                ..read::ReadOptions::default()
            },
        );
    }
    run_passthrough("sed", args)
}

pub fn run_nl(args: &[String]) -> Result<ProxyOutcome> {
    if let Some(path) = parse_nl(args) {
        return run_read(
            path,
            read::ReadOptions {
                line_numbers: true,
                raw: true,
                auto_aggressive: false,
                ..read::ReadOptions::default()
            },
        );
    }
    run_passthrough("nl", args)
}

fn run_read(path: &str, options: read::ReadOptions) -> Result<ProxyOutcome> {
    read::run(Path::new(path), &options)
}

fn run_passthrough(program: &str, args: &[String]) -> Result<ProxyOutcome> {
    let mut passthrough_args = Vec::with_capacity(args.len() + 1);
    passthrough_args.push(program.to_string());
    passthrough_args.extend(args.iter().cloned());
    passthrough::run(&passthrough_args)
}

fn parse_cat(args: &[String]) -> Option<(&str, bool)> {
    let mut line_numbers = false;
    let mut path = None;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "-n" | "--number" => {
                line_numbers = true;
                index += 1;
            }
            "--" => {
                path = single_remaining_path(args, index + 1)?;
                break;
            }
            value if value.starts_with('-') => return None,
            value => {
                set_single_path(&mut path, value)?;
                index += 1;
            }
        }
    }
    path.filter(|value| usable_path(value))
        .map(|value| (value, line_numbers))
}

fn parse_head(args: &[String]) -> Option<(&str, usize)> {
    let mut count = 10usize;
    let mut path = None;
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--" => {
                path = single_remaining_path(args, index + 1)?;
                break;
            }
            "-n" | "--lines" => {
                index += 1;
                count = parse_line_count(args.get(index)?.as_str())?;
                index += 1;
            }
            value if value.starts_with("--lines=") => {
                count = parse_line_count(value.strip_prefix("--lines=")?)?;
                index += 1;
            }
            value if value.starts_with("-n") && value.len() > 2 => {
                count = parse_line_count(&value[2..])?;
                index += 1;
            }
            value if short_count(value).is_some() => {
                count = short_count(value)?;
                index += 1;
            }
            value if value.starts_with('-') => return None,
            value => {
                set_single_path(&mut path, value)?;
                index += 1;
            }
        }
    }
    path.filter(|value| usable_path(value))
        .map(|value| (value, count))
}

fn parse_tail(args: &[String]) -> Option<(&str, TailMode)> {
    let mut mode = TailMode::Last(10);
    let mut path = None;
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--" => {
                path = single_remaining_path(args, index + 1)?;
                break;
            }
            "-n" | "--lines" => {
                index += 1;
                mode = parse_tail_count(args.get(index)?.as_str())?;
                index += 1;
            }
            value if value.starts_with("--lines=") => {
                mode = parse_tail_count(value.strip_prefix("--lines=")?)?;
                index += 1;
            }
            value if value.starts_with("-n") && value.len() > 2 => {
                mode = parse_tail_count(&value[2..])?;
                index += 1;
            }
            value if value.starts_with('+') => {
                mode = TailMode::From(parse_start_line(&value[1..])?);
                index += 1;
            }
            value if short_count(value).is_some() => {
                mode = TailMode::Last(short_count(value)?);
                index += 1;
            }
            value if value.starts_with('-') => return None,
            value => {
                set_single_path(&mut path, value)?;
                index += 1;
            }
        }
    }
    path.filter(|value| usable_path(value))
        .map(|value| (value, mode))
}

fn parse_sed(args: &[String]) -> Option<(&str, read::ReadRange)> {
    let mut quiet = false;
    let mut script = None;
    let mut path = None;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "-n" | "--quiet" | "--silent" => {
                quiet = true;
                index += 1;
            }
            "--" => {
                path = single_remaining_path(args, index + 1)?;
                break;
            }
            value if value.starts_with('-') => return None,
            value => {
                if script.is_none() {
                    script = Some(value);
                } else {
                    set_single_path(&mut path, value)?;
                }
                index += 1;
            }
        }
    }
    let script = script?;
    let path = path.filter(|value| usable_path(value))?;
    quiet.then(|| parse_sed_print_range(script).map(|range| (path, range)))?
}

fn parse_nl(args: &[String]) -> Option<&str> {
    let mut path = None;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "-ba" => {
                index += 1;
            }
            "-b" => {
                index += 1;
                if args.get(index).map(String::as_str) != Some("a") {
                    return None;
                }
                index += 1;
            }
            "--" => {
                path = single_remaining_path(args, index + 1)?;
                break;
            }
            value if value.starts_with('-') => return None,
            value => {
                set_single_path(&mut path, value)?;
                index += 1;
            }
        }
    }
    path.filter(|value| usable_path(value))
}

fn single_remaining_path(args: &[String], start: usize) -> Option<Option<&str>> {
    if args.len().checked_sub(start)? != 1 {
        return None;
    }
    Some(Some(args[start].as_str()))
}

fn set_single_path<'a>(path: &mut Option<&'a str>, value: &'a str) -> Option<()> {
    if path.replace(value).is_some() {
        return None;
    }
    Some(())
}

fn usable_path(path: &str) -> bool {
    !path.is_empty() && path != "-"
}

fn parse_line_count(value: &str) -> Option<usize> {
    value
        .chars()
        .all(|ch| ch.is_ascii_digit())
        .then(|| value.parse::<usize>().ok())?
}

fn parse_tail_count(value: &str) -> Option<TailMode> {
    if let Some(start) = value.strip_prefix('+') {
        return Some(TailMode::From(parse_start_line(start)?));
    }
    parse_line_count(value).map(TailMode::Last)
}

fn parse_start_line(value: &str) -> Option<usize> {
    let start = parse_line_count(value)?;
    (start > 0).then_some(start)
}

fn short_count(value: &str) -> Option<usize> {
    let count = value.strip_prefix('-')?;
    if count.is_empty() {
        return None;
    }
    parse_line_count(count)
}

fn parse_sed_print_range(script: &str) -> Option<read::ReadRange> {
    let body = script.trim().strip_suffix('p')?;
    if body.is_empty() {
        return None;
    }
    if let Some((start, end)) = body.split_once(',') {
        let start = parse_start_line(start.trim())?;
        let end = parse_sed_end_bound(end.trim())?;
        if let Some(end) = end {
            if end < start {
                return None;
            }
        }
        return Some(read::ReadRange {
            start: Some(start),
            end,
        });
    }
    let line = parse_start_line(body.trim())?;
    Some(read::ReadRange {
        start: Some(line),
        end: Some(line),
    })
}

fn parse_sed_end_bound(value: &str) -> Option<Option<usize>> {
    if value == "$" {
        return Some(None);
    }
    Some(Some(parse_start_line(value)?))
}

fn tail_last_range(path: &str, count: usize) -> Result<read::ReadRange> {
    if count == 0 {
        return Ok(read::ReadRange {
            start: Some(1),
            end: Some(0),
        });
    }
    let total = count_file_lines(path)?;
    let start = if count >= total { 1 } else { total - count + 1 };
    Ok(read::ReadRange {
        start: Some(start),
        end: None,
    })
}

fn count_file_lines(path: &str) -> Result<usize> {
    let file = fs::File::open(path).with_context(|| format!("failed to read {path}"))?;
    let reader = BufReader::new(file);
    let mut count = 0usize;
    for line in reader.lines() {
        line.with_context(|| format!("failed to read {path}"))?;
        count += 1;
    }
    Ok(count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TailMode {
    Last(usize),
    From(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lines.txt");
        fs::write(
            path,
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
        )
        .unwrap();
        temp
    }

    fn fixture_path(temp: &tempfile::TempDir) -> String {
        temp.path().join("lines.txt").display().to_string()
    }

    #[test]
    fn head_line_count_reads_first_window() {
        let temp = fixture();
        let path = fixture_path(&temp);
        let outcome = run_head(&["-n".into(), "3".into(), path]).unwrap();
        assert_eq!(outcome.stdout, "one\ntwo\nthree\n");
        assert_eq!(outcome.exit_code, 0);
    }

    #[test]
    fn cat_skill_file_preserves_exact_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("SKILL.md");
        let content = "# required instruction\n\n\nUse the full body.\n";
        fs::write(&path, content).unwrap();

        let outcome = run_cat(&[path.display().to_string()]).unwrap();

        assert_eq!(outcome.stdout, content);
        assert_eq!(outcome.exit_code, 0);
    }

    #[test]
    fn cat_diff_file_preserves_exact_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exact-commit.diff");
        let content = "diff --git a/src/app.js b/src/app.js\n--- a/src/app.js\n+++ b/src/app.js\n@@ -1 +1 @@\n-old\n+new\n";
        fs::write(&path, content).unwrap();

        let outcome = run_cat(&[path.display().to_string()]).unwrap();

        assert_eq!(outcome.stdout, content);
        assert_eq!(outcome.exit_code, 0);
    }

    #[test]
    fn sed_full_range_preserves_plan_json() {
        let temp = tempfile::tempdir().unwrap();
        let plan_dir = temp.path().join(".state").join("plans/example");
        fs::create_dir_all(&plan_dir).unwrap();
        let path = plan_dir.join("planSteps.json");
        let content = format!(
            r#"{{"planSteps":[{{"body":"{}"}}]}}"#,
            "exact-plan-body-".repeat(32)
        );
        assert!(content.chars().count() > 320);
        fs::write(&path, &content).unwrap();

        let outcome = run_sed(&["-n".into(), "1,$p".into(), path.display().to_string()]).unwrap();

        assert_eq!(outcome.stdout, content);
        assert!(serde_json::from_str::<serde_json::Value>(&outcome.stdout).is_ok());
        assert!(!outcome.stdout.contains("[truncated]"));
    }

    #[test]
    fn tail_line_count_reads_last_window() {
        let temp = fixture();
        let path = fixture_path(&temp);
        let outcome = run_tail(&["-n".into(), "2".into(), path]).unwrap();
        assert_eq!(outcome.stdout, "nine\nten\n");
    }

    #[test]
    fn tail_plus_line_reads_from_start_line() {
        let temp = fixture();
        let path = fixture_path(&temp);
        let outcome = run_tail(&["-n".into(), "+8".into(), path]).unwrap();
        assert_eq!(outcome.stdout, "eight\nnine\nten\n");
    }

    #[test]
    fn sed_quiet_print_range_reads_selected_lines() {
        let temp = fixture();
        let path = fixture_path(&temp);
        let outcome = run_sed(&["-n".into(), "2,4p".into(), path]).unwrap();
        assert_eq!(outcome.stdout, "two\nthree\nfour\n");
    }

    #[test]
    fn sed_range_preserves_long_jsx_element_lines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("panel.jsx");
        let jsx_line = r#"<header className="organization-chat-voice-transcript__header" data-voice-state={voiceState} data-transcript-mode={transcriptMode} data-transcript-source={sourceLabel} aria-label={voiceTranscriptLabel} title={voiceTranscriptLabel} role="banner"><span className="organization-chat-voice-transcript__speaker">{speakerName}</span><span className="organization-chat-voice-transcript__confidence" data-confidence={confidenceLabel}>{confidenceLabel}</span><button className="organization-chat-voice-transcript__copy" type="button" aria-label={copyLabel} onClick={copyTranscript}>Copy transcript</button></header>"#;
        assert!(jsx_line.chars().count() > 320);
        fs::write(&path, format!("before\n{jsx_line}\nafter\n")).unwrap();

        let outcome = run_sed(&["-n".into(), "2,2p".into(), path.display().to_string()]).unwrap();

        assert_eq!(outcome.stdout, format!("{jsx_line}\n"));
        assert!(!outcome.stdout.contains("[truncated]"));
        assert!(outcome.stdout.contains("Copy transcript"));
    }

    #[test]
    fn sed_range_preserves_long_regex_assertion_lines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("static_contract.mjs");
        let regex_terms = (0..40)
            .map(|index| format!("stableRouteMarker{index}"))
            .collect::<Vec<_>>()
            .join("|");
        let assertion_line = format!(
            r#"assert.match(sourceText, /settingsFrame|frame\.url|Open Seer diagnostics|service-health|surface-open|click\(|{regex_terms}|final-signal/);"#
        );
        assert!(assertion_line.chars().count() > 320);
        fs::write(&path, format!("before\n{assertion_line}\nafter\n")).unwrap();

        let outcome = run_sed(&["-n".into(), "2,2p".into(), path.display().to_string()]).unwrap();

        assert_eq!(outcome.stdout, format!("{assertion_line}\n"));
        assert!(!outcome.stdout.contains("[truncated]"));
        assert!(outcome.stdout.contains("stableRouteMarker39"));
    }

    #[test]
    fn sed_range_preserves_long_embedded_rust_source_literals() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("git_tests.rs");
        let embedded_block =
            "\\nif [ \\\"$1\\\" = \\\"diff\\\" ]; then\\nprintf 'diff line'\\nelse\\nexit 9\\nfi"
                .repeat(6);
        let source_line = format!("                \"#!/bin/sh{embedded_block}\",");
        assert!(source_line.chars().count() > 320);
        fs::write(&path, format!("before\n{source_line}\nafter\n")).unwrap();

        let outcome = run_sed(&["-n".into(), "2,2p".into(), path.display().to_string()]).unwrap();

        assert_eq!(outcome.stdout, format!("{source_line}\n"));
        assert!(!outcome.stdout.contains("[truncated]"));
        assert!(outcome.stdout.contains("exit 9\\nfi"));
    }

    #[test]
    fn sed_range_preserves_long_structured_rust_format_literals() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("archive_fixture.rs");
        let source_line = concat!(
            "        r#\"{{\"rawBytes\":{raw_bytes},\"rawChars\":{raw_chars},",
            "\"rawLines\":{raw_lines},\"rawTokens\":{raw_tokens},",
            "\"emittedBytes\":{emitted_bytes},\"emittedChars\":{emitted_chars},",
            "\"emittedLines\":{emitted_lines},\"emittedTokens\":{emitted_tokens},",
            "\"savedBytes\":{saved_bytes},\"savedChars\":{saved_chars},",
            "\"savedLines\":{saved_lines},\"savedTokens\":{saved_tokens}}}\"#,"
        )
        .to_string();
        fs::write(&path, format!("before\n{source_line}\nafter\n")).unwrap();

        let outcome = run_sed(&["-n".into(), "2,2p".into(), path.display().to_string()]).unwrap();

        assert_eq!(outcome.stdout, format!("{source_line}\n"));
        assert!(!outcome.stdout.contains("[truncated]"));
        assert!(outcome.stdout.contains("{saved_tokens}"));
    }

    #[test]
    fn sed_dollar_end_reads_through_file_end() {
        let temp = fixture();
        let path = fixture_path(&temp);
        let outcome = run_sed(&["-n".into(), "8,$p".into(), path]).unwrap();
        assert_eq!(outcome.stdout, "eight\nnine\nten\n");
    }

    #[test]
    fn cat_number_uses_read_line_number_format() {
        let temp = fixture();
        let path = fixture_path(&temp);
        let outcome = run_cat(&["--number".into(), path]).unwrap();
        assert!(outcome.stdout.contains("1 │ one"));
        assert!(outcome.stdout.contains("10 │ ten"));
    }

    #[test]
    fn nl_basic_uses_read_line_number_format() {
        let temp = fixture();
        let path = fixture_path(&temp);
        let outcome = run_nl(&["-ba".into(), path]).unwrap();
        assert!(outcome.stdout.contains("1 │ one"));
        assert!(outcome.stdout.contains("10 │ ten"));
    }

    #[test]
    fn nl_keeps_deep_lines_for_downstream_ranges() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lines.txt");
        let content = (1..=1200)
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, content).unwrap();

        let outcome = run_nl(&["-ba".into(), path.display().to_string()]).unwrap();

        assert!(outcome.stdout.contains("880 │ 880"));
        assert!(outcome.stdout.contains("1200 │ 1200"));
    }
}
