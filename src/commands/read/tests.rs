use super::*;
use std::fmt::Write as _;
use std::{fs, io::Write};
use tempfile::{tempdir, NamedTempFile};

use crate::support::test_support::{with_env_vars, write_executable};

#[test]
fn read_filters_rust_comment_only_lines() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        file,
        "// comment\nfn main() {{\n    println!(\"hi\");\n}}\n"
    )
    .unwrap();

    let output = run(file.path(), &ReadOptions::default()).unwrap();
    assert!(!output.stdout.contains("// comment"));
    assert!(output.stdout.contains("fn main()"));
}

#[test]
fn read_tail_lines_returns_last_lines() {
    let output = apply_window("a\nb\nc\nd", None, Some(2));
    assert_eq!(output, "c\nd");
}

#[test]
fn read_head_lines_returns_first_lines() {
    let output = apply_window("a\nb\nc\nd", Some(2), None);
    assert_eq!(output, "a\nb");
}

#[test]
fn line_numbers_are_added() {
    let output = format_with_line_numbers("alpha\nbeta");
    assert!(output.contains("1 │ alpha"));
    assert!(output.contains("2 │ beta"));
}

#[test]
fn line_numbers_can_start_from_original_line_offset() {
    let output = format_with_line_numbers_from("alpha\nbeta", 42);
    assert!(output.contains("42 │ alpha"));
    assert!(output.contains("43 │ beta"));
}

#[test]
fn aggressive_mode_keeps_signatures_not_bodies() {
    let content = "use std::fs;\n\npub fn alpha() {\n    println!(\"alpha\");\n}\n\npub struct Beta {\n    value: usize,\n}\n";
    let rendered = render_aggressive(content, Path::new("demo.rs"), Language::Rust, Some(20));
    assert!(rendered.contains("use std::fs;"));
    assert!(rendered.contains("pub fn alpha() {"));
    assert!(rendered.contains("pub struct Beta {"));
    assert!(!rendered.contains("println!"));
    assert!(!rendered.contains("value: usize"));
}

#[test]
fn auto_aggressive_triggers_for_large_content() {
    let content = "line\n".repeat(DEFAULT_AUTO_THRESHOLD_LINES + 1);
    assert!(should_auto_aggressive(&content));
}

#[test]
fn auto_aggressive_can_be_disabled() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    let mut content = String::new();
    content.push_str("pub fn alpha() {\n    println!(\"alpha\");\n}\n\n");
    content.push_str(&"// filler\n".repeat(DEFAULT_AUTO_THRESHOLD_LINES + 5));
    write!(file, "{content}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            auto_aggressive: false,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert!(output.stdout.contains("println!(\"alpha\")"));
}

#[test]
fn skill_files_are_exact_by_default() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("SKILL.md");
    let mut content = String::from("# required instruction comment\n\n\n");
    for index in 0..(DEFAULT_AUTO_THRESHOLD_LINES + 5) {
        writeln!(&mut content, "instruction_{index}").unwrap();
    }
    writeln!(
        &mut content,
        "long_instruction={}",
        "A".repeat(READ_LINE_PREVIEW_CHARS + 120)
    )
    .unwrap();
    fs::write(&path, &content).unwrap();

    let output = run(&path, &ReadOptions::default()).unwrap();

    assert_eq!(output.stdout, content);
}

#[test]
fn agents_files_are_exact_by_default() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("AGENTS.md");
    let content = "# operator rule\n\n\nDo not compact this instruction file.\n";
    fs::write(&path, content).unwrap();

    let output = run(&path, &ReadOptions::default()).unwrap();

    assert_eq!(output.stdout, content);
}

#[test]
fn markdown_files_are_exact_by_default() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("product-spec.md");
    let mut content = String::from("# CX spec\n\n");
    for index in 0..(DEFAULT_AUTO_THRESHOLD_LINES + 5) {
        writeln!(&mut content, "requirement_{index}").unwrap();
    }
    writeln!(
        &mut content,
        "long_markdown_line={}",
        "M".repeat(READ_LINE_PREVIEW_CHARS + 120)
    )
    .unwrap();
    fs::write(&path, &content).unwrap();

    let output = run(&path, &ReadOptions::default()).unwrap();

    assert_eq!(output.stdout, content);
}

#[test]
fn text_document_files_are_exact_by_default() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("operator-notes.txt");
    let mut content = String::from("CX notes\n\n");
    for index in 0..(DEFAULT_AUTO_THRESHOLD_LINES + 5) {
        writeln!(&mut content, "note_{index}").unwrap();
    }
    writeln!(
        &mut content,
        "long_text_line={}",
        "T".repeat(READ_LINE_PREVIEW_CHARS + 120)
    )
    .unwrap();
    fs::write(&path, &content).unwrap();

    let output = run(&path, &ReadOptions::default()).unwrap();

    assert_eq!(output.stdout, content);
}

#[test]
fn diff_artifacts_are_exact_by_default() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("exact-commit.diff");
    let long_added_line = format!(
        "+const evidence = \"{}\";",
        "D".repeat(READ_LINE_PREVIEW_CHARS + 120)
    );
    let content = format!(
        "diff --git a/src/app.js b/src/app.js\n--- a/src/app.js\n+++ b/src/app.js\n@@ -1 +1 @@\n-old\n{long_added_line}\n"
    );
    fs::write(&path, &content).unwrap();

    let output = run(&path, &ReadOptions::default()).unwrap();

    assert_eq!(output.stdout, content);
}

#[test]
fn plan_json_is_exact_by_default() {
    let temp = tempdir().unwrap();
    let plan_dir = temp.path().join(".state").join("plans/38b40715962c7b9d");
    fs::create_dir_all(&plan_dir).unwrap();
    let path = plan_dir.join("planSteps.json");
    let content = format!(
        r#"{{"planSteps":[{{"step":"{}","status":"in_progress"}}]}}"#,
        "plan-requirement-".repeat(40)
    );
    assert!(content.chars().count() > READ_LINE_PREVIEW_CHARS);
    fs::write(&path, &content).unwrap();

    let output = run(&path, &ReadOptions::default()).unwrap();

    assert_eq!(output.stdout, content);
    assert!(serde_json::from_str::<serde_json::Value>(&output.stdout).is_ok());
    assert!(!output.stdout.contains("[truncated]"));
}

#[test]
fn plan_json_ranges_do_not_truncate_long_lines() {
    let temp = tempdir().unwrap();
    let plan_dir = temp.path().join(".state").join("plans/example");
    fs::create_dir_all(&plan_dir).unwrap();
    let path = plan_dir.join("planSteps.json");
    let long_line = format!(
        r#"{{"body":"{}"}}"#,
        "R".repeat(READ_LINE_PREVIEW_CHARS + 120)
    );
    fs::write(&path, format!("{long_line}\n")).unwrap();

    let output = run(
        &path,
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: None,
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{long_line}\n"));
    assert!(serde_json::from_str::<serde_json::Value>(&output.stdout).is_ok());
}

#[test]
fn skill_ranges_do_not_truncate_huge_lines() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("SKILL.md");
    let long_line = format!("instruction={}", "B".repeat(READ_LINE_PREVIEW_CHARS + 120));
    fs::write(&path, format!("{long_line}\nnext\n")).unwrap();

    let output = run(
        &path,
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{long_line}\n"));
}

#[test]
fn markdown_ranges_do_not_truncate_huge_lines() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("product-spec.md");
    let long_line = format!(
        "markdown_requirement={}",
        "D".repeat(READ_LINE_PREVIEW_CHARS + 120)
    );
    fs::write(&path, format!("intro\n{long_line}\nnext\n")).unwrap();

    let output = run(
        &path,
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(2),
                end: Some(2),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{long_line}\n"));
}

#[test]
fn explicit_head_keeps_normal_mode_even_for_large_files() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    let mut content = String::new();
    content.push_str("pub fn alpha() {\n    println!(\"alpha\");\n}\n\n");
    content.push_str(&"// filler\n".repeat(DEFAULT_AUTO_THRESHOLD_LINES + 5));
    write!(file, "{content}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            head_lines: Some(4),
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert!(output.stdout.contains("println!(\"alpha\")"));
    assert!(output.stdout.lines().count() <= 4);
    assert!(output.stdout.starts_with("pub fn alpha()"));
}

#[test]
fn explicit_tail_keeps_normal_mode_even_for_large_files() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    for index in 0..(DEFAULT_AUTO_THRESHOLD_LINES + 5) {
        writeln!(file, "let line_{index} = {index};").unwrap();
    }

    let output = run(
        file.path(),
        &ReadOptions {
            tail_lines: Some(3),
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        output.stdout,
        format!(
            "let line_{} = {};\nlet line_{} = {};\nlet line_{} = {};",
            DEFAULT_AUTO_THRESHOLD_LINES + 2,
            DEFAULT_AUTO_THRESHOLD_LINES + 2,
            DEFAULT_AUTO_THRESHOLD_LINES + 3,
            DEFAULT_AUTO_THRESHOLD_LINES + 3,
            DEFAULT_AUTO_THRESHOLD_LINES + 4,
            DEFAULT_AUTO_THRESHOLD_LINES + 4
        )
    );
}

#[test]
fn explicit_range_keeps_exact_lines_even_for_large_files() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "pub fn alpha() {{").unwrap();
    writeln!(file, "    println!(\"alpha\");").unwrap();
    writeln!(file, "}}").unwrap();
    for index in 0..(DEFAULT_AUTO_THRESHOLD_LINES + 5) {
        writeln!(file, "// filler {index}").unwrap();
    }

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(3),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        output.stdout,
        "pub fn alpha() {\n    println!(\"alpha\");\n}\n"
    );
}

#[test]
fn explicit_range_preserves_original_line_numbers() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    for index in 1..=12 {
        writeln!(file, "line_{index}").unwrap();
    }

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(9),
                end: Some(11),
            }),
            line_numbers: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert_eq!(output.stdout, " 9 │ line_9\n10 │ line_10\n11 │ line_11");
}

#[test]
fn explicit_range_truncates_huge_blob_literals_by_default() {
    let mut file = NamedTempFile::with_suffix(".mjs").unwrap();
    writeln!(
        file,
        "const blob = \"{}\";",
        "A".repeat(READ_LINE_PREVIEW_CHARS + 120)
    )
    .unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("[truncated]"));
    assert!(!output
        .stdout
        .contains(&"A".repeat(READ_LINE_PREVIEW_CHARS + 40)));
}

#[test]
fn explicit_range_truncates_wrapped_blob_string_literals() {
    let wrapped_blob = (0..8)
        .map(|_| "A".repeat(80))
        .collect::<Vec<_>>()
        .join("\\n");
    let source_line = format!("\"{wrapped_blob}\"");
    assert!(source_line.chars().count() > READ_LINE_PREVIEW_CHARS);
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "{source_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("[truncated]"));
    assert!(output.stdout.len() < source_line.len());
}

#[test]
fn explicit_range_preserves_long_source_definition_lines() {
    let mut file = NamedTempFile::with_suffix(".js").unwrap();
    let long_definition = "function responseProofRow({ id = \"\", label = \"\", text = \"\", match = \"\", normalizedMatch = \"\", source = \"\", artifact = \"\", artifactKind = \"\", artifactLabel = \"\", attemptArtifact = \"\", attemptArtifactLabel = \"\", attemptId = \"\", artifactStatus = \"\", statusLabel = \"\", byteCount = 0, payloadCheck = \"\", payloadArtifact = \"\", payloadArtifactKind = \"\", payloadText = \"\", executionRequested = \"\", executed = \"\", executionMode = \"\", executionState = \"\", executionBlockedReason = \"\", executionExitCode = \"\" } = {}) {";
    assert!(long_definition.chars().count() > READ_LINE_PREVIEW_CHARS);
    writeln!(file, "{long_definition}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{long_definition}\n"));
    assert!(!output.stdout.contains("[truncated]"));
    assert!(output.stdout.contains("executionBlockedReason"));
    assert!(output.stdout.contains("executionExitCode"));
}

#[test]
fn explicit_range_preserves_long_embedded_source_literal_lines() {
    let embedded_block =
        "\\nif [ \\\"$1\\\" = \\\"diff\\\" ]; then\\nprintf 'diff line'\\nelse\\nexit 9\\nfi"
            .repeat(6);
    let source_line = format!("                \"#!/bin/sh{embedded_block}\",");
    assert!(source_line.chars().count() > READ_LINE_PREVIEW_CHARS);
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "{source_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{source_line}\n"));
    assert!(!output.stdout.contains("[truncated]"));
    assert!(output.stdout.contains("exit 9\\nfi"));
}

#[test]
fn explicit_range_preserves_long_structured_rust_format_literals() {
    let source_line = concat!(
        "        r#\"{{\"rawBytes\":{raw_bytes},\"rawChars\":{raw_chars},",
        "\"rawLines\":{raw_lines},\"rawTokens\":{raw_tokens},",
        "\"emittedBytes\":{emitted_bytes},\"emittedChars\":{emitted_chars},",
        "\"emittedLines\":{emitted_lines},\"emittedTokens\":{emitted_tokens},",
        "\"savedBytes\":{saved_bytes},\"savedChars\":{saved_chars},",
        "\"savedLines\":{saved_lines},\"savedTokens\":{saved_tokens}}}\"#,"
    )
    .to_string();
    assert!(source_line.chars().count() > READ_LINE_PREVIEW_CHARS);
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "{source_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{source_line}\n"));
    assert!(!output.stdout.contains("[truncated]"));
    assert!(output.stdout.contains("{emitted_tokens}"));
    assert!(output.stdout.contains("{saved_tokens}"));
}

#[test]
fn explicit_range_preserves_long_cpp_initializer_lines_without_shape_heuristics() {
    let description = "Set true when the current user request requires a new or bound plan before repository work. With no bound plan, sync_context returns createRequiredPlan with shouldStop and mustBindBeforeWork; only plan preflight, allowed plan-authoring files, and plan persistence may proceed until a plan is bound. ";
    let source_line = format!(
        "              {{\"description\", \"{}\"}}}},",
        description.repeat(2)
    );
    assert!(source_line.chars().count() > READ_LINE_PREVIEW_CHARS);
    let mut file = NamedTempFile::with_suffix(".cpp").unwrap();
    writeln!(file, "{source_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{source_line}\n"));
    assert!(!output.stdout.contains("[truncated]"));
    assert!(output.stdout.contains("mustBindBeforeWork"));
}

#[test]
fn generated_source_ranges_keep_generic_long_line_bounding() {
    let temp = tempdir().unwrap();
    let generated = temp.path().join("generated");
    fs::create_dir_all(&generated).unwrap();
    let path = generated.join("command_registry.cpp");
    let description = "Set true when the current user request requires a new or bound plan before repository work. With no bound plan, sync_context returns createRequiredPlan with shouldStop and mustBindBeforeWork; only plan preflight, allowed plan-authoring files, and plan persistence may proceed until a plan is bound. ";
    let source_line = format!(
        "              {{\"description\", \"{}\"}}}},",
        description.repeat(2)
    );
    fs::write(&path, format!("{source_line}\n")).unwrap();

    let output = run(
        &path,
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("[truncated]"));
    assert!(output.stdout.len() < source_line.len());
}

#[test]
fn explicit_range_still_truncates_structured_literals_with_blob_tokens() {
    let blob = "A".repeat(READ_LINE_PREVIEW_CHARS);
    let source_line =
        format!("        r#\"{{\"kind\":\"fixture\",\"payload\":\"{blob}\",\"ok\":true}}\"#;");
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "{source_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("[truncated]"));
    assert!(output.stdout.len() < source_line.len());
}

#[test]
fn explicit_range_preserves_long_css_declaration_lines() {
    let mut file = NamedTempFile::with_suffix(".css").unwrap();
    let css_line = ".global-rail__item{display:grid;grid-template-columns:minmax(0,1fr) auto;align-items:center;gap:10px;padding:12px 14px;border:1px solid var(--cx-shell-global-rail-border,rgba(148,163,184,.22));background:linear-gradient(135deg,var(--cx-shell-global-rail-surface,rgba(15,23,42,.86)),var(--cx-shell-global-rail-surface-raised,rgba(30,41,59,.72)));box-shadow:0 18px 60px rgba(2,6,23,.36);color:var(--cx-shell-global-rail-text,#e5e7eb);transition:background .16s ease,border-color .16s ease,transform .16s ease;}";
    assert!(css_line.chars().count() > READ_LINE_PREVIEW_CHARS);
    writeln!(file, "{css_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{css_line}\n"));
    assert!(!output.stdout.contains("[truncated]"));
    assert!(output.stdout.contains("--cx-shell-global-rail-text"));
    assert!(output.stdout.contains("transform .16s ease"));
}

#[test]
fn explicit_range_preserves_long_jsx_element_lines() {
    let mut file = NamedTempFile::with_suffix(".jsx").unwrap();
    let jsx_line = r#"<header className="organization-chat-voice-transcript__header" data-voice-state={voiceState} data-transcript-mode={transcriptMode} data-transcript-source={sourceLabel} aria-label={voiceTranscriptLabel} title={voiceTranscriptLabel} role="banner"><span className="organization-chat-voice-transcript__speaker">{speakerName}</span><span className="organization-chat-voice-transcript__confidence" data-confidence={confidenceLabel}>{confidenceLabel}</span><button className="organization-chat-voice-transcript__copy" type="button" aria-label={copyLabel} onClick={copyTranscript}>Copy transcript</button></header>"#;
    assert!(jsx_line.chars().count() > READ_LINE_PREVIEW_CHARS);
    writeln!(file, "{jsx_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{jsx_line}\n"));
    assert!(!output.stdout.contains("[truncated]"));
    assert!(output
        .stdout
        .contains("organization-chat-voice-transcript__copy"));
    assert!(output.stdout.contains("Copy transcript"));
}

#[test]
fn explicit_range_preserves_long_regex_assertion_lines() {
    let mut file = NamedTempFile::with_suffix(".mjs").unwrap();
    let regex_terms = (0..40)
        .map(|index| format!("stableRouteMarker{index}"))
        .collect::<Vec<_>>()
        .join("|");
    let assertion_line = format!(
        r#"assert.match(sourceText, /settingsFrame|frame\.url|Open Seer diagnostics|service-health|surface-open|click\(|{regex_terms}|final-signal/);"#
    );
    assert!(assertion_line.chars().count() > READ_LINE_PREVIEW_CHARS);
    writeln!(file, "{assertion_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{assertion_line}\n"));
    assert!(!output.stdout.contains("[truncated]"));
    assert!(output.stdout.contains("frame\\.url"));
    assert!(output.stdout.contains("stableRouteMarker39"));
}

#[test]
fn explicit_range_truncates_css_blob_literals_by_default() {
    let mut file = NamedTempFile::with_suffix(".css").unwrap();
    writeln!(
        file,
        ".global-rail__icon{{background-image:url(\"data:image/png;base64,{}\");color:var(--cx-shell-global-rail-text,#e5e7eb);}}",
        "A".repeat(READ_LINE_PREVIEW_CHARS + 120)
    )
    .unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("[truncated]"));
    assert!(!output
        .stdout
        .contains(&"A".repeat(READ_LINE_PREVIEW_CHARS + 40)));
}

#[test]
fn explicit_range_truncates_jsx_blob_literals_by_default() {
    let mut file = NamedTempFile::with_suffix(".jsx").unwrap();
    writeln!(
        file,
        "<img className=\"organization-chat-avatar\" alt=\"avatar\" src={{\"data:image/png;base64,{}\"}} data-owner=\"voice-transcript\" />",
        "A".repeat(READ_LINE_PREVIEW_CHARS + 120)
    )
    .unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("[truncated]"));
    assert!(!output
        .stdout
        .contains(&"A".repeat(READ_LINE_PREVIEW_CHARS + 40)));
}

#[test]
fn explicit_range_raw_preserves_huge_lines() {
    let long_line = format!(
        "const blob = \"{}\";",
        "A".repeat(READ_LINE_PREVIEW_CHARS + 120)
    );
    let mut file = NamedTempFile::with_suffix(".mjs").unwrap();
    writeln!(file, "{long_line}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            raw: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, format!("{long_line}\n"));
}

#[test]
fn head_window_truncates_huge_lines_by_default() {
    let mut file = NamedTempFile::with_suffix(".mjs").unwrap();
    writeln!(
        file,
        "const blob = \"{}\";",
        "B".repeat(READ_LINE_PREVIEW_CHARS + 120)
    )
    .unwrap();
    writeln!(file, "const other = 1;").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            head_lines: Some(1),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("[truncated]"));
    assert!(!output
        .stdout
        .contains(&"B".repeat(READ_LINE_PREVIEW_CHARS + 40)));
}

#[test]
fn explicit_open_ended_range_reads_to_end() {
    let output = apply_line_range(
        "a\nb\nc\nd",
        ReadRange {
            start: Some(3),
            end: None,
        },
    );
    assert_eq!(output, "c\nd");
}

#[test]
fn explicit_range_preserves_selected_blank_lines() {
    let output = apply_line_range(
        "a\n\nc\nd",
        ReadRange {
            start: Some(2),
            end: Some(3),
        },
    );
    assert_eq!(output, "\nc\n");
}

#[test]
fn explicit_range_preserves_trailing_newline() {
    let output = apply_line_range(
        "a\nb\n",
        ReadRange {
            start: Some(1),
            end: None,
        },
    );
    assert_eq!(output, "a\nb\n");
}

#[test]
fn explicit_open_ended_range_run_reads_to_end() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    for index in 1..=5 {
        writeln!(file, "line_{index}").unwrap();
    }

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(3),
                end: None,
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, "line_3\nline_4\nline_5\n");
    assert!(output
        .observation
        .as_ref()
        .is_some_and(|observation| observation.uses_preserved_stream_termination()));
}

#[test]
fn explicit_range_preserves_crlf_and_unterminated_final_lines() {
    let mut file = NamedTempFile::with_suffix(".txt").unwrap();
    write!(file, "one\r\ntwo\r\nthree").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(2),
                end: None,
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert_eq!(output.stdout, "two\r\nthree");
    assert!(output
        .observation
        .as_ref()
        .is_some_and(|observation| observation.uses_preserved_stream_termination()));
}

#[test]
fn transformed_range_uses_cx_stream_termination() {
    let mut file = NamedTempFile::with_suffix(".txt").unwrap();
    write!(file, "one\r\ntwo").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(1),
                end: None,
            }),
            line_numbers: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.contains("1 │ one"));
    assert!(!output
        .observation
        .as_ref()
        .is_some_and(|observation| observation.uses_preserved_stream_termination()));
}

#[test]
fn explicit_reversed_range_struct_returns_empty() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "line_1").unwrap();
    writeln!(file, "line_2").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            line_range: Some(ReadRange {
                start: Some(3),
                end: Some(2),
            }),
            ..ReadOptions::default()
        },
    )
    .unwrap();

    assert!(output.stdout.is_empty());
}

#[test]
fn line_numbers_apply_after_explicit_head_window() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "pub fn alpha() {{").unwrap();
    writeln!(file, "    println!(\"alpha\");").unwrap();
    writeln!(file, "}}").unwrap();
    writeln!(file, "pub fn beta() {{}}").unwrap();

    let output = run(
        file.path(),
        &ReadOptions {
            head_lines: Some(3),
            line_numbers: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert!(output.stdout.contains("1 │ pub fn alpha() {"));
    assert!(output.stdout.contains("2 │     println!(\"alpha\");"));
    assert!(output.stdout.contains("3 │ }"));
}

#[test]
fn max_lines_caps_aggressive_output() {
    let content = (0..50)
        .map(|index| format!("pub fn item_{index}() {{}}\n"))
        .collect::<String>();
    let rendered = render_aggressive(
        content.as_str(),
        Path::new("demo.rs"),
        Language::Rust,
        Some(10),
    );
    assert!(rendered.contains("... ["));
}

#[test]
fn cap_lines_preserves_content_at_exact_cap() {
    assert_eq!(window::cap_lines("a\nb\n", 2), "a\nb\n");
}

#[test]
fn cap_lines_falls_back_after_cap() {
    let capped = window::cap_lines("a\nb\nc", 2);
    assert!(capped.contains("a"));
    assert!(capped.contains("c"));
    assert!(capped.contains("omitted"));
}

#[test]
fn run_respects_explicit_aggressive_mode_and_max_lines() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    for index in 0..40 {
        writeln!(file, "pub fn item_{index}() {{ println!(\"{index}\"); }}").unwrap();
    }

    let output = run(
        file.path(),
        &ReadOptions {
            mode: ReadMode::Aggressive,
            max_lines: Some(8),
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert!(output.stdout.contains("pub fn item_0()"));
    assert!(output.stdout.contains("... ["));
    assert!(!output.stdout.contains("println!"));
}

#[test]
fn smart_summary_local_shape_is_compact() {
    let content = "use std::fs;\n\n/// Handles demo operations.\npub fn alpha() {}\n";
    let summary = render_local_smart_summary(content, Path::new("demo.rs"), Language::Rust);
    assert!(summary.contains("Purpose:"));
    assert!(summary.contains("Key exports/entrypoints:"));
    assert!(summary.contains("Likely touch points:"));
    assert!(summary.lines().count() <= 5);
}

#[test]
fn smart_mode_uses_plugin_summary_when_available() {
    let temp = tempdir().unwrap();
    let plugin = temp.path().join("smart-plugin");
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    write_executable(
        temp.path(),
        "smart-plugin",
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"summary\":\"plugin summary\"}'\n",
    );
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "pub fn alpha() {{}}").unwrap();

    with_env_vars(
        &[
            (
                "CX_SMART_READ_COMMAND",
                Some(plugin.to_string_lossy().as_ref()),
            ),
            ("HOME", Some(home.to_string_lossy().as_ref())),
        ],
        || {
            let output = run(
                file.path(),
                &ReadOptions {
                    mode: ReadMode::Smart,
                    ..ReadOptions::default()
                },
            )
            .unwrap();
            assert_eq!(output.stdout, "plugin summary");
            assert!(output.stderr.is_empty());
        },
    );
}

#[test]
fn smart_mode_falls_back_to_local_summary_when_plugin_fails() {
    let temp = tempdir().unwrap();
    let plugin = temp.path().join("smart-plugin");
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    write_executable(
        temp.path(),
        "smart-plugin",
        "#!/bin/sh\nprintf 'boom' >&2\nexit 1\n",
    );
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(file, "/// Demo.\npub fn alpha() {{}}").unwrap();

    with_env_vars(
        &[
            (
                "CX_SMART_READ_COMMAND",
                Some(plugin.to_string_lossy().as_ref()),
            ),
            ("HOME", Some(home.to_string_lossy().as_ref())),
        ],
        || {
            let output = run(
                file.path(),
                &ReadOptions {
                    mode: ReadMode::Smart,
                    ..ReadOptions::default()
                },
            )
            .unwrap();
            assert!(output.stdout.contains("Purpose:"));
            assert!(output.stderr.contains("cx smart-read plugin failed: boom"));
        },
    );
}
