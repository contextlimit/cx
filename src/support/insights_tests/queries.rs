use super::*;

#[test]
fn savings_distribution_uses_nearest_rank_and_full_filtered_dataset() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            for saved_tokens in 0..=10 {
                let raw = OutputObservation::from_metrics(
                    "read",
                    TextMetrics {
                        bytes: saved_tokens,
                        chars: saved_tokens,
                        lines: saved_tokens,
                        tokens: saved_tokens,
                    },
                );
                record_invocation(&InvocationRecord {
                    command: "read",
                    exit_code: 0,
                    raw: Some(&raw),
                    emitted: TextMetrics::default(),
                })
                .unwrap();
            }
            let outlier = OutputObservation::from_metrics(
                "git diff",
                TextMetrics {
                    bytes: 100,
                    chars: 100,
                    lines: 100,
                    tokens: 100,
                },
            );
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&outlier),
                emitted: TextMetrics::default(),
            })
            .unwrap();

            let distribution = savings_distribution().unwrap();
            assert_eq!(distribution.invocations, 12);
            assert_eq!(distribution.saving_invocations, 11);
            assert_eq!(distribution.total_saved_tokens, 155);
            assert_eq!(distribution.all_p50_saved_tokens, 5);
            assert_eq!(distribution.all_p95_saved_tokens, 100);
            assert_eq!(distribution.all_p99_saved_tokens, 100);
            assert_eq!(distribution.saving_p50_saved_tokens, 6);
            assert_eq!(distribution.saving_p95_saved_tokens, 100);
            assert_eq!(distribution.saving_p99_saved_tokens, 100);
            assert_eq!(distribution.largest_saved_tokens, 100);
            assert_eq!(distribution.top_ten_saved_tokens, 154);
            assert_eq!(distribution.saved_tokens_excluding_largest(), 55);
            assert_eq!(distribution.saved_tokens_excluding_top_ten(), 1);

            let read_distribution = savings_distribution_filtered(CommandFilter {
                command_root: Some("read"),
                command: None,
            })
            .unwrap();
            assert_eq!(read_distribution.invocations, 11);
            assert_eq!(read_distribution.total_saved_tokens, 55);
            assert_eq!(read_distribution.all_p50_saved_tokens, 5);
            assert_eq!(read_distribution.all_p95_saved_tokens, 10);
            assert_eq!(read_distribution.largest_saved_tokens, 10);
            assert_eq!(read_distribution.top_ten_saved_tokens, 55);
            assert_eq!(read_distribution.saved_tokens_excluding_top_ten(), 0);
        },
    );
}

#[test]
fn filtered_queries_scope_totals_and_invocations_by_root_or_command() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let diff_raw = OutputObservation::from_text("git diff", &"d\n".repeat(8));
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&diff_raw),
                emitted: TextMetrics::from_text("d\n"),
            })
            .unwrap();
            let log_raw = OutputObservation::from_text("git log", &"l\n".repeat(6));
            record_invocation(&InvocationRecord {
                command: "git log",
                exit_code: 1,
                raw: Some(&log_raw),
                emitted: TextMetrics::from_text("l\n"),
            })
            .unwrap();
            let read_raw = OutputObservation::from_text("read source", &"r\n".repeat(5));
            record_invocation(&InvocationRecord {
                command: "read",
                exit_code: 0,
                raw: Some(&read_raw),
                emitted: TextMetrics::from_text("r\n"),
            })
            .unwrap();

            let git_filter = CommandFilter {
                command_root: Some("git"),
                command: None,
            };
            let git_overall = overall_totals_filtered(git_filter).unwrap();
            assert_eq!(git_overall.invocations, 2);
            assert_eq!(git_overall.failures, 1);

            let git_leaf_totals = command_totals_at_level_filtered(
                CommandLevel::Command,
                CommandTotalSort::Invocations,
                10,
                git_filter,
            )
            .unwrap();
            assert_eq!(git_leaf_totals.len(), 2);
            assert!(git_leaf_totals
                .iter()
                .all(|total| total.command.starts_with("git ")));

            let git_recent = recent_invocations_filtered(10, git_filter).unwrap();
            assert_eq!(git_recent.len(), 2);
            assert!(git_recent
                .iter()
                .all(|invocation| invocation.process == "git"));

            let diff_filter = CommandFilter {
                command_root: None,
                command: Some("git diff"),
            };
            let largest_diff =
                largest_invocations_filtered(SavingsSort::Lines, 10, diff_filter).unwrap();
            assert_eq!(largest_diff.len(), 1);
            assert_eq!(largest_diff[0].command, "git diff");

            let read_filter = CommandFilter {
                command_root: Some("read"),
                command: None,
            };
            let days = daily_totals_filtered(10, read_filter).unwrap();
            assert_eq!(days.len(), 1);
            assert_eq!(days[0].invocations, 1);
        },
    );
}

#[test]
fn failure_coverage_distinguishes_unknown_silent_linked_and_output_gaps() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            set_insight_setting("record_invocations", "true").unwrap();
            set_insight_setting("record_failures", "true").unwrap();
            set_insight_setting("record_failure_responses", "true").unwrap();

            record_coverage_failure(
                "covered",
                "covered run",
                "covered output\n[full output: ~/.cx/cache/failures/covered/1.log]",
                "covered output\n",
            );
            record_coverage_failure("silent", "silent run", "", "");
            record_coverage_failure("gap", "gap run", "gap output\n", "gap output\n");
            for _ in 0..2 {
                let raw = OutputObservation::from_text("unknown", "unknown output\n");
                record_invocation_with_context(
                    &InvocationRecord {
                        command: "unknown run",
                        exit_code: 9,
                        raw: Some(&raw),
                        emitted: TextMetrics::from_text("unknown output\n"),
                    },
                    Some(&InvocationContext {
                        process: "unknown",
                        command: "unknown run",
                        argv_json: r#"["cx","--","unknown","run"]"#,
                        emitted_response: Some("unknown output\n"),
                    }),
                )
                .unwrap();
            }
            record_failure_detail(&FailureDetailRecord {
                command_family: "orphan run",
                command_line: "orphan run",
                exit_code: 8,
                cx_response: "orphan output\n[full output: ~/.cx/cache/failures/orphan/1.log]",
                raw_source: Some("orphan"),
                raw_response: Some("orphan output\n"),
            })
            .unwrap();

            let rows =
                failure_coverage_at_level_filtered(CommandLevel::Root, CommandFilter::default())
                    .unwrap();
            let by_command = rows
                .iter()
                .map(|row| (row.command.as_str(), row))
                .collect::<std::collections::BTreeMap<_, _>>();

            let covered = by_command["covered"];
            assert_eq!(covered.failed_invocations, 1);
            assert_eq!(covered.linked_invocations, 1);
            assert_eq!(covered.artifact_linked_detail_rows, 1);
            assert_eq!(covered.output_gap_detail_rows, 0);
            assert_eq!(covered.unknown_invocations, 0);

            let silent = by_command["silent"];
            assert_eq!(silent.silent_detail_rows, 1);
            assert_eq!(silent.output_bearing_detail_rows, 0);
            assert_eq!(silent.output_gap_detail_rows, 0);

            let gap = by_command["gap"];
            assert_eq!(gap.output_bearing_detail_rows, 1);
            assert_eq!(gap.output_gap_detail_rows, 1);
            assert_eq!(gap.artifact_linked_detail_rows, 0);

            let unknown = by_command["unknown"];
            assert_eq!(unknown.failed_invocations, 2);
            assert_eq!(unknown.detail_rows, 0);
            assert_eq!(unknown.unknown_invocations, 2);

            let orphan = by_command["orphan"];
            assert_eq!(orphan.failed_invocations, 0);
            assert_eq!(orphan.detail_rows, 1);
            assert_eq!(orphan.orphan_detail_rows, 1);
            assert_eq!(orphan.artifact_linked_detail_rows, 1);

            let filtered = failure_coverage_at_level_filtered(
                CommandLevel::Command,
                CommandFilter {
                    command_root: Some("gap"),
                    command: Some("gap run"),
                },
            )
            .unwrap();
            assert_eq!(filtered.len(), 1);
            assert_eq!(filtered[0].command, "gap run");
            assert_eq!(filtered[0].output_gap_detail_rows, 1);
        },
    );
}

fn record_coverage_failure(
    process: &str,
    command_family: &str,
    cx_response: &str,
    raw_response: &str,
) {
    let raw = OutputObservation::from_text(process, raw_response);
    record_invocation_with_context_and_failure(
        &InvocationRecord {
            command: command_family,
            exit_code: 7,
            raw: Some(&raw),
            emitted: TextMetrics::from_text(cx_response),
        },
        Some(&InvocationContext {
            process,
            command: command_family,
            argv_json: "[]",
            emitted_response: Some(cx_response),
        }),
        Some(&FailureDetailRecord {
            command_family,
            command_line: command_family,
            exit_code: 7,
            cx_response,
            raw_source: Some(process),
            raw_response: Some(raw_response),
        }),
    )
    .unwrap();
}

#[test]
fn expansion_queries_preserve_positive_deltas_and_wrapper_reason() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let status_raw = OutputObservation::from_text("git status", "")
                .with_expansion_reason("status-summary");
            let status_emitted = TextMetrics::from_text("Clean working tree");
            record_invocation(&InvocationRecord {
                command: "git status",
                exit_code: 0,
                raw: Some(&status_raw),
                emitted: status_emitted,
            })
            .unwrap();

            let diff_raw = OutputObservation::from_text("git diff", &"line\n".repeat(20));
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&diff_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();

            let overall = overall_totals().unwrap();
            assert_eq!(overall.invocations, 2);
            assert_eq!(overall.expansions, 1);
            assert_eq!(overall.expanded, status_emitted);
            assert!(overall.saved.tokens > 0);

            let rows = expansion_invocations(10).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].command, "git status");
            assert_eq!(rows[0].expanded, status_emitted);
            assert_eq!(rows[0].expansion_reason, "status-summary");
            assert_eq!(rows[0].saved, TextMetrics::default());

            let totals =
                command_totals_at_level(CommandLevel::Command, CommandTotalSort::Invocations, 10)
                    .unwrap();
            let status = totals
                .iter()
                .find(|total| total.command == "git status")
                .unwrap();
            assert_eq!(status.expansions, 1);
            assert_eq!(status.expanded, status_emitted);
            assert_eq!(status.best_expanded_tokens, status_emitted.tokens);

            let daily = daily_totals(1).unwrap();
            assert_eq!(daily[0].expansions, 1);
            assert_eq!(daily[0].expanded, status_emitted);

            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let stored = connection
                .query_row(
                    "SELECT expansion_reason, expanded_tokens FROM command_invocations
                     WHERE command_family = 'git status'",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
                )
                .unwrap();
            assert_eq!(
                stored,
                ("status-summary".to_string(), status_emitted.tokens)
            );
        },
    );
}

#[test]
fn record_command_report_writes_recent_quality_issue() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let receipt = record_command_report(&CommandReportRecord {
                command: "cx grep route|path src",
                command_family: "grep",
                command_shape: "",
                command_shape_hash: "",
                issue_kind: "suspicious_output",
                note: "bare alternation returned zero matches",
            })
            .unwrap();
            assert!(receipt.recorded);
            assert_eq!(receipt.id, 1);
            assert_eq!(receipt.command_family, "grep");
            assert_eq!(receipt.total_reports, 1);
            assert_eq!(receipt.family_reports, 1);

            let reports = recent_command_reports(10).unwrap();
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].command_root, "grep");
            assert_eq!(reports[0].command_family, "grep");
            assert_eq!(reports[0].issue_kind, "suspicious_output");
            assert!(reports[0].command.contains("route|path"));
        },
    );
}

#[test]
fn record_failure_detail_skips_grep_no_match_and_records_real_error() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let no_match = record_failure_detail(&FailureDetailRecord {
                command_family: "grep basic",
                command_line: "cx grep needle src",
                exit_code: 1,
                cx_response: "0 matches for 'needle'",
                raw_source: Some("grep"),
                raw_response: Some(""),
            })
            .unwrap();
            assert!(!no_match);

            let recorded = record_failure_detail(&FailureDetailRecord {
                command_family: "rg extended",
                command_line: "cx rg '(' src",
                exit_code: 2,
                cx_response: "[full output: ~/.cx/cache/failures/grep/1.log]\nregex parse error",
                raw_source: Some("grep"),
                raw_response: Some("regex parse error: unclosed group\n"),
            })
            .unwrap();
            assert!(recorded);

            let details = recent_failure_details(10).unwrap();
            assert_eq!(details.len(), 1);
            assert_eq!(details[0].command_family, "rg extended");
            assert_eq!(details[0].command_line, "cx rg '(' src");
            assert_eq!(details[0].exit_code, 2);
            assert!(details[0].cx_response.contains("[full output:"));
            assert_eq!(details[0].raw_source, "grep");
            assert!(details[0].raw_response.contains("unclosed group"));
        },
    );
}

#[test]
fn search_families_treat_exit_one_as_no_match_and_larger_codes_as_failures() {
    assert!(!exit_code_is_actionable_failure("grep basic", 1));
    assert!(!exit_code_is_actionable_failure("rg extended", 1));
    assert!(exit_code_is_actionable_failure("grep fixed", 2));
    assert!(exit_code_is_actionable_failure("rg fixed", 2));

    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            for (command, exit_code) in [
                ("grep basic", 1),
                ("rg extended", 1),
                ("grep fixed", 2),
                ("rg fixed", 2),
            ] {
                record_invocation(&InvocationRecord {
                    command,
                    exit_code,
                    raw: None,
                    emitted: TextMetrics::from_text("search result"),
                })
                .unwrap();
            }

            let totals = overall_totals().unwrap();
            assert_eq!(totals.invocations, 4);
            assert_eq!(totals.failures, 2);

            let commands =
                command_totals_at_level(CommandLevel::Command, CommandTotalSort::Invocations, 10)
                    .unwrap();
            assert_eq!(commands.len(), 4);
            assert_eq!(commands.iter().map(|row| row.failures).sum::<u64>(), 2);
        },
    );
}
