use super::*;

#[test]
fn format_matches_groups_by_file() {
    let formatted = output::format_matches(
        "needle",
        &[String::from(".")],
        "src/main.rs:10:let needle = true;\nsrc/lib.rs:5:needle()\n",
        &GrepOptions::default(),
    )
    .unwrap();
    assert!(formatted.contains("2 matches"));
    assert!(formatted.contains("[file] src/lib.rs"));
    assert!(formatted.contains("[file] src/main.rs"));
}

#[test]
fn format_matches_respects_max_results() {
    let formatted = output::format_matches(
        "needle",
        &[String::from(".")],
        "src/main.rs:10:let needle = true;\nsrc/lib.rs:5:needle()\nsrc/app.rs:8:needle = 2\n",
        &GrepOptions {
            max_results: Some(2),
            ..GrepOptions::default()
        },
    )
    .unwrap();
    assert!(formatted.contains("2 shown of 3 matches"));
    assert!(formatted.contains("[file] src/lib.rs"));
    assert!(formatted.contains("+1 more matches hidden by --max-results"));
}

#[test]
fn format_matches_handles_single_path_content_starting_with_colon() {
    let formatted = output::format_matches(
        "needle",
        &[String::from("src/bin/cx.rs")],
        "2153::Status { args } => cx::commands::git::run_status(args)?,\n",
        &GrepOptions::default(),
    )
    .unwrap();
    assert!(formatted.contains("[file] src/bin/cx.rs"));
    assert!(formatted.contains(":Status { args }"));
    assert!(!formatted.contains("[file] 2153"));
}

#[test]
fn format_matches_truncates_huge_match_lines() {
    let huge = format!(
        "src/blob.mjs:8:{}",
        "A".repeat(output::MATCH_LINE_PREVIEW_CHARS + 80)
    );
    let formatted =
        output::format_matches("blob", &[String::from(".")], &huge, &GrepOptions::default())
            .unwrap();
    assert!(formatted.contains("[file] src/blob.mjs"));
    assert!(formatted.contains("[truncated]"));
    assert!(!formatted.contains(&"A".repeat(output::MATCH_LINE_PREVIEW_CHARS + 40)));
}

#[test]
fn format_matches_preserves_long_structured_source_lines() {
    let source_line = concat!(
        "r#\"{{\"rawBytes\":{raw_bytes},\"rawChars\":{raw_chars},",
        "\"rawLines\":{raw_lines},\"rawTokens\":{raw_tokens},",
        "\"emittedBytes\":{emitted_bytes},\"emittedChars\":{emitted_chars},",
        "\"emittedLines\":{emitted_lines},\"emittedTokens\":{emitted_tokens},",
        "\"savedBytes\":{saved_bytes},\"savedChars\":{saved_chars},",
        "\"savedLines\":{saved_lines},\"savedTokens\":{saved_tokens}}}\"#,"
    )
    .to_string();
    assert!(source_line.chars().count() > output::MATCH_LINE_PREVIEW_CHARS);
    let raw = format!("src/archive.rs:330:{source_line}");

    let formatted = output::format_matches(
        "rawBytes",
        &[String::from(".")],
        &raw,
        &GrepOptions::default(),
    )
    .unwrap();

    assert!(formatted.contains(&source_line));
    assert!(!formatted.contains("[truncated]"));
    assert!(formatted.contains("{saved_tokens}"));
}

#[test]
fn raw_match_output_preserves_location_prefixed_structured_source_lines() {
    let source_line = concat!(
        "r#\"{{\"rawBytes\":{raw_bytes},\"rawChars\":{raw_chars},",
        "\"rawLines\":{raw_lines},\"rawTokens\":{raw_tokens},",
        "\"emittedBytes\":{emitted_bytes},\"emittedChars\":{emitted_chars},",
        "\"emittedLines\":{emitted_lines},\"emittedTokens\":{emitted_tokens},",
        "\"savedBytes\":{saved_bytes},\"savedChars\":{saved_chars},",
        "\"savedLines\":{saved_lines},\"savedTokens\":{saved_tokens}}}\"#,"
    )
    .to_string();
    let raw = format!("src/archive.rs:330:{source_line}");

    assert_eq!(output::truncate_output_lines(&raw), raw);
}

#[test]
fn detects_document_only_match_output() {
    let plan_path = format!("{}/plans/38b40715962c7b9d/planSteps.json", ".state");
    assert!(output::output_is_document_only(
        &[String::from(".")],
        "docs/spec.md:4:needle\nnotes/guide.txt:8:needle\n",
        false,
    ));
    assert!(!output::output_is_document_only(
        &[String::from(".")],
        "docs/spec.md:4:needle\nsrc/main.rs:8:needle\n",
        false,
    ));
    assert!(output::output_is_document_only(
        &[String::from("docs/spec.rst")],
        "4:needle\n5-more context\n",
        false,
    ));
    assert!(output::output_is_document_only(
        &[String::from(".")],
        "docs/operator-guide.md-4-context before\n--\ndocs/operator-guide.md:5:needle\n",
        false,
    ));
    assert!(output::output_is_compaction_protected_only(
        &[String::from(".")],
        &format!("{plan_path}:1:needle\n"),
        false,
    ));
    assert!(!output::output_is_compaction_protected_only(
        &[String::from(".")],
        "package.json:1:needle\n",
        false,
    ));
}

#[test]
fn truncate_output_lines_bounds_raw_fallback_lines() {
    let raw = format!(
        "{}\nshort",
        "B".repeat(output::MATCH_LINE_PREVIEW_CHARS + 120)
    );
    let bounded = output::truncate_output_lines(&raw);
    let first = bounded.lines().next().unwrap();
    assert!(first.contains("[truncated]"));
    assert!(first.chars().count() <= output::MATCH_LINE_PREVIEW_CHARS);
    assert_eq!(bounded.lines().nth(1), Some("short"));
}

#[test]
fn truncate_output_lines_matches_lines_semantics_for_trailing_newline() {
    assert_eq!(output::truncate_output_lines("one\ntwo\n"), "one\ntwo");
    assert_eq!(output::truncate_output_lines("one\n\n"), "one\n");
    assert_eq!(output::truncate_output_lines("\none"), "\none");
}

#[test]
fn format_stdout_preserves_small_exact_search_results() {
    let stdout = "330:        let rawBytes = metrics.raw_bytes;";
    let formatted = format_stdout(
        "rawBytes",
        &[String::from("src/archive.rs")],
        stdout,
        &GrepOptions::default(),
        false,
    );

    assert_eq!(formatted, stdout);
}

#[test]
fn format_stdout_groups_results_above_small_exact_limit() {
    let stdout = (1..=SMALL_EXACT_RESULT_LINES + 1)
        .map(|line| format!("src/main.rs:{line}:    let needle_{line} = true;"))
        .collect::<Vec<_>>()
        .join("\n");
    let formatted = format_stdout(
        "needle",
        &[String::from("src")],
        &stdout,
        &GrepOptions::default(),
        false,
    );

    assert!(formatted.contains("9 matches for 'needle'"));
    assert!(formatted.contains("[file] src/main.rs"));
    assert!(formatted.contains("... +1 more in file"));
}

#[test]
fn format_stdout_keeps_max_results_accounting_for_small_raw_output() {
    let formatted = format_stdout(
        "needle",
        &[String::from("src")],
        "src/main.rs:7:    let needle = true;",
        &GrepOptions {
            max_results: Some(1),
            ..GrepOptions::default()
        },
        false,
    );

    assert!(formatted.contains("1 matches for 'needle'"));
    assert!(formatted.contains("[file] src/main.rs"));
}

#[test]
fn format_files_with_matches_lists_files() {
    let formatted = output::format_files_with_matches("needle", "src/a.rs\nsrc/b.rs\n", Some(1));
    assert!(formatted.contains("1 shown of 2 files with matches"));
    assert!(formatted.contains("+1 more files hidden by --max-results"));
    assert!(formatted.contains("2 files with matches"));
    assert!(formatted.contains("src/a.rs"));
    assert!(!formatted.contains("src/b.rs"));
}

#[test]
fn format_files_with_matches_ignores_blank_lines() {
    let formatted = output::format_files_with_matches("needle", "\nsrc/a.rs\n  \nsrc/b.rs\n", None);
    assert!(formatted.contains("2 files with matches"));
    assert!(!formatted.contains("3 files with matches"));
}

#[test]
fn format_file_list_respects_max_results() {
    let formatted = output::format_file_list("src/a.rs\nsrc/b.rs\nsrc/c.rs\n", Some(2));
    assert!(formatted.contains("2 shown of 3 files"));
    assert!(formatted.contains("src/a.rs"));
    assert!(formatted.contains("src/b.rs"));
    assert!(formatted.contains("+1 more files hidden by --max-results"));
}

#[test]
fn format_file_list_allows_zero_max_results() {
    let formatted = output::format_file_list("src/a.rs\nsrc/b.rs\n", Some(0));
    assert!(formatted.contains("0 shown of 2 files"));
    assert!(formatted.contains("+2 more files hidden by --max-results"));
    assert!(!formatted.contains("src/a.rs"));
}

#[test]
fn format_stdout_preserves_document_search_results_by_default() {
    let stdout = (1..=12)
        .map(|index| {
            format!(
                "docs/spec.txt:{index}:needle_{index:02}_{}",
                "A".repeat(output::MATCH_LINE_PREVIEW_CHARS + 80)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let formatted = format_stdout(
        "needle",
        &[String::from("docs")],
        &stdout,
        &GrepOptions::default(),
        false,
    );

    assert_eq!(formatted.lines().count(), 12);
    assert!(formatted.contains("docs/spec.txt:12:needle_12"));
    assert!(!formatted.contains("[file]"));
    assert!(!formatted.contains("[truncated]"));
    assert!(!formatted.contains("more in file"));
}

#[test]
fn format_stdout_preserves_tabular_text_search_results_by_default() {
    for extension in ["tsv", "csv"] {
        let stdout = (1..=12)
            .map(|index| format!("audit/report.{extension}:{index}:field_{index:02}\tvalue"))
            .collect::<Vec<_>>()
            .join("\n");

        let formatted = format_stdout(
            "field",
            &[format!("audit/report.{extension}")],
            &stdout,
            &GrepOptions::default(),
            false,
        );

        assert_eq!(formatted, stdout);
        assert!(!formatted.contains("[file]"));
        assert!(!formatted.contains("more in file"));
    }
}

#[test]
fn format_stdout_can_compact_document_results_when_setting_enabled() {
    let stdout = (1..=12)
        .map(|index| format!("docs/spec.txt:{index}:needle_{index:02}"))
        .collect::<Vec<_>>()
        .join("\n");

    let formatted = format_stdout(
        "needle",
        &[String::from("docs")],
        &stdout,
        &GrepOptions::default(),
        true,
    );

    assert!(formatted.contains("[file] docs/spec.txt"));
    assert!(formatted.contains("... +4 more in file"));
}

#[test]
fn format_stdout_never_compacts_plan_json_results() {
    let plan_path = format!("{}/plans/38b40715962c7b9d/planSteps.json", ".state");
    let stdout = (1..=12)
        .map(|index| format!("{plan_path}:{index}:needle_{index:02}"))
        .collect::<Vec<_>>()
        .join("\n");

    let formatted = format_stdout(
        "needle",
        std::slice::from_ref(&plan_path),
        &stdout,
        &GrepOptions::default(),
        true,
    );

    assert_eq!(formatted, stdout);
    assert!(!formatted.contains("[file]"));
    assert!(!formatted.contains("more in file"));
}
