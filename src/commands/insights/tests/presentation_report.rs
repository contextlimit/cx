use super::*;

#[test]
fn run_presentation_renders_seeded_database_sections() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let home = temp.path().join("home");
    let artifact_dir = home.join(".cx/cache/failures/grep");
    fs::create_dir_all(&artifact_dir).unwrap();
    fs::write(artifact_dir.join("001.log"), "grep failure").unwrap();
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let raw = OutputObservation::from_text("git diff", "one\ntwo\nthree\nfour\n");
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&raw),
                emitted: TextMetrics::from_text("one\n"),
            })
            .unwrap();
            let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(12));
            record_invocation(&InvocationRecord {
                command: "grep",
                exit_code: 2,
                raw: Some(&grep_raw),
                emitted: TextMetrics::from_text("match\n"),
            })
            .unwrap();

            let output = run_presentation(3).unwrap();
            assert_presentation_output(&output.stdout);
        },
    );
}

fn assert_presentation_output(output: &str) {
    for expected in [
        "cx insights: presentation summary",
        "Executive headlines",
        "Metric scorecard",
        "Raw output:",
        "Emitted output:",
        "Saved output:",
        "Failed invocations: 1 (50.0%)",
        "Context-window equivalents saved:",
        "Savings distribution:",
        "All-invocation saved-token percentiles:",
        "Top 10 invocations:",
        "Saved excluding top 10 invocations:",
        "Slide outline",
        "Speaker bullets",
        "Recommended focus areas",
        "Operational health",
        "Artifact-linked details",
        "Demo commands",
        "cx insights impact --limit 3",
        "Evidence snapshot",
        "Top command roots",
        "Top command families",
        "Largest single saves",
        "Daily totals",
        "git diff",
        "grep",
    ] {
        assert!(
            output.contains(expected),
            "missing `{expected}` in\n{output}"
        );
    }
}

#[test]
fn run_report_renders_compact_terminal_brief() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let home = temp.path().join("home");
    let artifact_dir = home.join(".cx/cache/failures/grep");
    fs::create_dir_all(&artifact_dir).unwrap();
    fs::write(artifact_dir.join("001.log"), "grep failure").unwrap();
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let raw = OutputObservation::from_text("git diff", &"one\n".repeat(24));
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&raw),
                emitted: TextMetrics::from_text("one\n"),
            })
            .unwrap();
            let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(12));
            record_invocation(&InvocationRecord {
                command: "grep",
                exit_code: 2,
                raw: Some(&grep_raw),
                emitted: TextMetrics::from_text("match\n"),
            })
            .unwrap();

            let output = run_report(3, CommandFilter::default()).unwrap();
            assert!(output.stdout.contains("cx insights: report"));
            assert!(output.stdout.contains("Executive brief:"));
            assert!(output.stdout.contains("Proof points:"));
            assert!(output.stdout.contains("Recent activity:"));
            assert!(output.stdout.contains("Recommended next moves:"));
            assert!(output.stdout.contains("Operator commands:"));
            assert!(output.stdout.contains("Top family: `git diff`"));
            assert!(output
                .stdout
                .contains("Savings outside the top 10 invocations total"));
            assert!(output
                .stdout
                .contains("Most recent measured command: `grep`"));
            assert!(output
                .stdout
                .contains("cx insights export --format json --limit 3"));
            assert!(output.stdout.contains("grep | 2 | grep"));
        },
    );
}

#[test]
fn run_reports_renders_quality_report_totals_and_recent_rows() {
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
            for (command, command_family) in [
                ("cx git diff -- src", "git diff"),
                ("cx git status --short", "git status"),
                ("cx grep route|path src", "grep"),
            ] {
                record_command_report(&CommandReportRecord {
                    command,
                    command_family,
                    command_shape: "",
                    command_shape_hash: "",
                    issue_kind: "suspicious_output",
                    note: "test report",
                })
                .unwrap();
            }

            let output = run_reports(
                5,
                CommandLevel::Root,
                CommandFilter {
                    command_root: Some("git"),
                    command: None,
                },
                None,
            )
            .unwrap();
            assert!(output
                .stdout
                .contains("cx insights: command-quality reports"));
            assert!(output.stdout.contains("Level: command roots"));
            assert!(output.stdout.contains("Status filter: all"));
            assert!(output.stdout.contains("Filter: root=git"));
            assert!(output.stdout.contains("Lifecycle: 2 total | 2 open"));
            assert!(output.stdout.contains("git | 2 | 2"));
            assert!(output.stdout.contains("Recent reports:"));
            assert!(output
                .stdout
                .contains("git | git status | open | - | - | no-match | suspicious_output"));
            assert!(output
                .stdout
                .contains("git | git diff | open | - | - | no-match | suspicious_output"));
            assert!(!output
                .stdout
                .contains("grep | grep | open | suspicious_output"));
        },
    );
}

#[test]
fn run_recent_renders_seeded_invocations_in_chronological_order() {
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
            let read_raw = OutputObservation::from_text("read source", "a\nb\nc\n");
            record_invocation(&InvocationRecord {
                command: "read",
                exit_code: 0,
                raw: Some(&read_raw),
                emitted: TextMetrics::from_text("a\n"),
            })
            .unwrap();
            let grep_raw = OutputObservation::from_text("grep source", "one\ntwo\n");
            record_invocation(&InvocationRecord {
                command: "grep",
                exit_code: 1,
                raw: Some(&grep_raw),
                emitted: TextMetrics::from_text("one\n"),
            })
            .unwrap();

            let output = run_recent(5, CommandFilter::default()).unwrap();
            assert!(output.stdout.contains("cx insights: recent invocations"));
            let grep_index = output
                .stdout
                .find("grep | grep | grep | 1 | grep source")
                .unwrap();
            let read_index = output
                .stdout
                .find("read | read | read | 0 | read source")
                .unwrap();
            assert!(grep_index < read_index);
        },
    );
}

#[test]
fn run_recent_can_filter_by_command_root() {
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
            let diff_raw = OutputObservation::from_text("git diff", &"line\n".repeat(8));
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&diff_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();
            let read_raw = OutputObservation::from_text("read source", &"line\n".repeat(8));
            record_invocation(&InvocationRecord {
                command: "read",
                exit_code: 0,
                raw: Some(&read_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();

            let output = run_recent(
                5,
                CommandFilter {
                    command_root: Some("git"),
                    command: None,
                },
            )
            .unwrap();
            assert!(output.stdout.contains("Filter: root=git"));
            assert!(output.stdout.contains("git | git diff"));
            assert!(!output.stdout.contains("read | read"));
        },
    );
}

#[test]
fn run_expansions_lists_only_expanded_rows_with_reasons() {
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
            record_invocation(&InvocationRecord {
                command: "git status",
                exit_code: 0,
                raw: Some(&status_raw),
                emitted: TextMetrics::from_text("Clean working tree"),
            })
            .unwrap();
            let read_raw = OutputObservation::from_text("read source", &"line\n".repeat(8));
            record_invocation(&InvocationRecord {
                command: "read",
                exit_code: 0,
                raw: Some(&read_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();

            let output = run_expansions(
                5,
                CommandFilter {
                    command_root: Some("git"),
                    command: None,
                },
            )
            .unwrap();
            assert!(output.stdout.contains("cx insights: expanded invocations"));
            assert!(output.stdout.contains("Filter: root=git"));
            assert!(output.stdout.contains("git status"));
            assert!(output.stdout.contains("status-summary"));
            assert!(!output.stdout.contains("read source"));
        },
    );
}

#[test]
fn run_top_can_render_root_level_totals() {
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
            let diff_raw = OutputObservation::from_text("git diff", &"line\n".repeat(20));
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&diff_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();
            let log_raw = OutputObservation::from_text("git log", &"line\n".repeat(10));
            record_invocation(&InvocationRecord {
                command: "git log",
                exit_code: 0,
                raw: Some(&log_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();

            let output = run_top(CommandTotalSort::Tokens, CommandLevel::Root, 5).unwrap();
            assert!(output
                .stdout
                .contains("cx insights: top command roots by saved tokens"));
            assert!(output.stdout.contains("git | 2 | 0"));
            assert!(!output.stdout.contains("git diff |"));
        },
    );
}

#[test]
fn run_failures_can_group_by_command_root() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let db_path = temp.path().join("db.sqlite");
    let artifact_dir = home.join(".cx/cache/failures/git");
    fs::create_dir_all(&artifact_dir).unwrap();
    fs::write(artifact_dir.join("001.log"), "git failure").unwrap();
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let diff_raw = OutputObservation::from_text("git diff", &"line\n".repeat(8));
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 1,
                raw: Some(&diff_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();
            let log_raw = OutputObservation::from_text("git log", &"line\n".repeat(6));
            record_invocation(&InvocationRecord {
                command: "git log",
                exit_code: 2,
                raw: Some(&log_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();

            let output = run_failures(5, CommandLevel::Root, CommandFilter::default()).unwrap();
            assert!(output.stdout.contains("Level: command roots"));
            assert!(output.stdout.contains("git | 2 | 0 | 2"));
            assert!(!output.stdout.contains("git diff |"));
        },
    );
}

#[test]
fn run_impact_renders_headlines_and_bullets() {
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
            let raw = OutputObservation::from_text("node check", &"line\n".repeat(12));
            record_invocation(&InvocationRecord {
                command: "node check",
                exit_code: 0,
                raw: Some(&raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();

            let output = run_impact(3, 10).unwrap();
            assert!(output.stdout.contains("cx insights: impact scorecard"));
            assert!(output.stdout.contains("Context-window equivalents saved"));
            assert!(output.stdout.contains("Command concentration"));
            assert!(output.stdout.contains("Presentation bullets"));
            assert!(output.stdout.contains("`node check`"));
        },
    );
}

#[test]
fn run_recommend_renders_focus_areas() {
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
            let node_raw = OutputObservation::from_text("node check", &"line\n".repeat(40));
            record_invocation(&InvocationRecord {
                command: "node check",
                exit_code: 0,
                raw: Some(&node_raw),
                emitted: TextMetrics::from_text("line\n"),
            })
            .unwrap();
            let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(12));
            record_invocation(&InvocationRecord {
                command: "grep",
                exit_code: 2,
                raw: Some(&grep_raw),
                emitted: TextMetrics::from_text("match\n"),
            })
            .unwrap();
            for _ in 0..3 {
                let read_raw = OutputObservation::from_text("read source", "a\nb\nc\n");
                record_invocation(&InvocationRecord {
                    command: "read",
                    exit_code: 0,
                    raw: Some(&read_raw),
                    emitted: TextMetrics::from_text("a\n"),
                })
                .unwrap();
            }

            let output = run_recommend(6).unwrap();
            assert!(output.stdout.contains("cx insights: recommendations"));
            assert!(output.stdout.contains("Protect `node check`"));
            assert!(output.stdout.contains("failure-heavy `grep`"));
            assert!(output.stdout.contains("highest-frequency `read`"));
            assert!(output.stdout.contains("largest-save proof"));
        },
    );
}

#[test]
fn build_recommendations_prioritizes_missing_artifacts() {
    let failed_total = CommandTotalInsight {
        command: "grep".to_string(),
        invocations: 3,
        failures: 2,
        expansions: 0,
        raw: TextMetrics::default(),
        emitted: TextMetrics::default(),
        saved: TextMetrics::default(),
        expanded: TextMetrics::default(),
        best_saved_chars: 0,
        best_saved_tokens: 0,
        best_expanded_tokens: 0,
    };
    let analysis = RecommendationAnalysis {
        database: "test.sqlite".to_string(),
        overall: OverallInsight {
            invocations: 3,
            failures: 2,
            expansions: 0,
            raw: TextMetrics::default(),
            emitted: TextMetrics::default(),
            saved: TextMetrics::default(),
            expanded: TextMetrics::default(),
        },
        savings_distribution: insights::SavingsDistributionInsight::default(),
        by_tokens: Vec::new(),
        by_invocations: Vec::new(),
        by_failures: vec![failed_total.clone()],
        failure_focus: vec![FailureFocus {
            total: failed_total,
            coverage: FailureCoverageInsight {
                command: "grep".to_string(),
                failed_invocations: 2,
                detail_rows: 2,
                linked_detail_rows: 2,
                linked_invocations: 2,
                output_bearing_detail_rows: 2,
                output_gap_detail_rows: 2,
                response_evidence_available: true,
                artifact_reference_available: true,
                ..FailureCoverageInsight::default()
            },
            artifact_summary: FailureArtifactSummary {
                tool_name: "grep".to_string(),
                display_dir: "~/.cx/cache/failures/grep".to_string(),
                count: 0,
                latest_display_path: None,
            },
        }],
        failure_coverage: FailureCoverageSummary {
            failing_groups: 1,
            failed_invocations: 2,
            detail_rows: 2,
            linked_detail_rows: 2,
            output_bearing_detail_rows: 2,
            output_gap_detail_rows: 2,
            groups_with_output_gaps: 1,
            ..FailureCoverageSummary::default()
        },
        largest_invocations: Vec::new(),
    };

    let recommendations = build_recommendations(&analysis, 2);
    assert_eq!(
        recommendations[0].title,
        "Repair artifact coverage for `grep`"
    );
    assert!(recommendations[0]
        .evidence
        .contains("2 output-bearing failure details lack artifact references"));
    assert!(recommendations[0]
        .action
        .contains("artifact-linking helper"));
}

#[test]
fn run_failures_renders_artifact_context() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let db_path = temp.path().join("db.sqlite");
    let artifact_dir = home.join(".cx/cache/failures/grep");
    fs::create_dir_all(&artifact_dir).unwrap();
    fs::write(artifact_dir.join("001.log"), "old").unwrap();
    fs::write(artifact_dir.join("002.log"), "new").unwrap();

    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(12));
            record_invocation(&InvocationRecord {
                command: "grep",
                exit_code: 2,
                raw: Some(&grep_raw),
                emitted: TextMetrics::from_text("match\n"),
            })
            .unwrap();
            let read_raw = OutputObservation::from_text("read source", "a\nb\nc\n");
            record_invocation(&InvocationRecord {
                command: "read",
                exit_code: 0,
                raw: Some(&read_raw),
                emitted: TextMetrics::from_text("a\n"),
            })
            .unwrap();

            let output = run_failures(5, CommandLevel::Command, CommandFilter::default()).unwrap();
            assert!(output.stdout.contains("cx insights: failures"));
            assert!(output.stdout.contains("grep | 1 | 0 | 1 | 0 | 0 | 2"));
            assert!(output.stdout.contains("~/.cx/cache/failures/grep/002.log"));
            assert!(!output.stdout.contains("artifact coverage warnings:"));
            assert!(output.stdout.contains("failure-detail coverage notes:"));
            assert!(output.stdout.contains(
                "unknown: `grep` has 1 failed invocations without linked failure-detail evidence"
            ));
            assert!(!output.stdout.contains("read |"));
        },
    );
}

#[test]
fn run_failures_warns_when_failures_have_no_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let db_path = temp.path().join("db.sqlite");

    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(12));
            record_invocation_with_context_and_failure(
                &InvocationRecord {
                    command: "grep",
                    exit_code: 2,
                    raw: Some(&grep_raw),
                    emitted: TextMetrics::from_text("match\n"),
                },
                None,
                Some(&FailureDetailRecord {
                    command_family: "grep",
                    command_line: "cx grep malformed src",
                    exit_code: 2,
                    cx_response: "regex parse error\n",
                    raw_source: Some("grep"),
                    raw_response: Some("regex parse error\n"),
                }),
            )
            .unwrap();

            let output = run_failures(5, CommandLevel::Command, CommandFilter::default()).unwrap();
            assert!(output.stdout.contains("grep | 1 | 1 | 0 | 1 | 0 | 0"));
            assert!(output.stdout.contains("artifact coverage warnings:"));
            assert!(output.stdout.contains(
                "warning: `grep` has 1 output-bearing failure details without artifact references"
            ));
        },
    );
}
