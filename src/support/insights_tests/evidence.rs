use std::path::Path;

use super::*;

const EVIDENCE_ARTIFACT_REF: &str = "~/.cx/cache/failures/grep/evidence.log.gz";
const EVIDENCE_SECRET: &str = "sk-abcdefghijklmnopqrstuvwxyz0123456789";

#[test]
fn response_previews_are_opt_in_and_reports_link_immutable_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let repo = home.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let home_text = home.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home_text.as_str())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_THREAD_ID", Some("evidence-thread")),
        ],
        || run_report_evidence_contract(&home, &repo),
    );
}

#[test]
fn reports_do_not_guess_between_ambiguous_shape_matches() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("reports.sqlite");
    let database_text = database.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(database_text.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            set_insight_setting("record_invocations", "true").unwrap();
            set_insight_setting("record_command_text", "false").unwrap();
            set_insight_setting("record_command_shape", "true").unwrap();
            set_insight_setting("record_response_previews", "false").unwrap();
            for pattern in ["alpha", "beta"] {
                let command = format!("rg {pattern} src");
                let argv_json = format!(r#"["cx","--","rg","{pattern}","src"]"#);
                record_invocation_with_context(
                    &InvocationRecord {
                        command: "grep",
                        exit_code: 0,
                        raw: None,
                        emitted: TextMetrics::from_text("match\n"),
                    },
                    Some(&InvocationContext {
                        process: "grep",
                        command: &command,
                        argv_json: &argv_json,
                        emitted_response: Some("match\n"),
                    }),
                )
                .unwrap();
            }

            let alpha_args = ["rg", "alpha", "src"].map(str::to_string);
            let beta_args = ["rg", "beta", "src"].map(str::to_string);
            let shape = crate::support::redaction::argument_shape_join(&alpha_args);
            assert_eq!(
                shape,
                crate::support::redaction::argument_shape_join(&beta_args)
            );
            let connection = rusqlite::Connection::open(&database).unwrap();
            let rows = connection
                .prepare(
                    "SELECT command, command_shape, command_shape_hash
                     FROM command_invocations ORDER BY id",
                )
                .unwrap()
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].1, shape);
            assert_eq!(rows[1].1, shape);
            assert_eq!(rows[0].2, rows[1].2);
            drop(connection);
            record_command_report(&CommandReportRecord {
                command: "rg alpha src",
                command_family: "grep",
                command_shape: &shape,
                command_shape_hash: &crate::support::redaction::stable_shape_hash(&shape),
                issue_kind: "incorrect_output",
                note: "ambiguous shape fixture",
            })
            .unwrap();

            let report = recent_command_reports(1).unwrap().pop().unwrap();
            assert_eq!(report.invocation_id, None);
            assert_eq!(report.evidence_kind, "no-match");
            assert_eq!(report.command_shape, shape);
        },
    );
}

#[test]
fn reports_do_not_borrow_unique_shape_evidence_when_command_text_was_recorded() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("reports.sqlite");
    let database_text = database.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(database_text.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            set_insight_setting("record_invocations", "true").unwrap();
            set_insight_setting("record_command_text", "true").unwrap();
            set_insight_setting("record_command_shape", "true").unwrap();
            record_invocation_with_context(
                &InvocationRecord {
                    command: "sh",
                    exit_code: 0,
                    raw: None,
                    emitted: TextMetrics::from_text("thread-id\n"),
                },
                Some(&InvocationContext {
                    process: "sh",
                    command: "bash -lc 'printenv CODEX_THREAD_ID'",
                    argv_json: r#"["cx","--","bash","-lc","printenv CODEX_THREAD_ID"]"#,
                    emitted_response: Some("thread-id\n"),
                }),
            )
            .unwrap();

            let report_args = ["bash", "-lc", "generated-artifact-hash-check"].map(str::to_string);
            let shape = crate::support::redaction::argument_shape_join(&report_args);
            record_command_report(&CommandReportRecord {
                command: "bash -lc generated-artifact-hash-check",
                command_family: "sh",
                command_shape: &shape,
                command_shape_hash: &crate::support::redaction::stable_shape_hash(&shape),
                issue_kind: "incorrect_output",
                note: "must not borrow a different retained command",
            })
            .unwrap();

            let report = recent_command_reports(1).unwrap().pop().unwrap();
            assert_eq!(report.invocation_id, None);
            assert_eq!(report.evidence_kind, "no-match");
        },
    );
}

#[test]
fn reports_can_use_one_unique_shape_when_command_text_was_not_recorded() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("reports.sqlite");
    let database_text = database.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(database_text.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            set_insight_setting("record_invocations", "true").unwrap();
            set_insight_setting("record_command_text", "false").unwrap();
            set_insight_setting("record_command_shape", "true").unwrap();
            record_invocation_with_context(
                &InvocationRecord {
                    command: "grep",
                    exit_code: 0,
                    raw: None,
                    emitted: TextMetrics::from_text("match\n"),
                },
                Some(&InvocationContext {
                    process: "grep",
                    command: "rg alpha src",
                    argv_json: r#"["cx","--","rg","alpha","src"]"#,
                    emitted_response: Some("match\n"),
                }),
            )
            .unwrap();

            let report_args = ["rg", "beta", "src"].map(str::to_string);
            let shape = crate::support::redaction::argument_shape_join(&report_args);
            record_command_report(&CommandReportRecord {
                command: "rg beta src",
                command_family: "grep",
                command_shape: &shape,
                command_shape_hash: &crate::support::redaction::stable_shape_hash(&shape),
                issue_kind: "incorrect_output",
                note: "unique shape without retained command text",
            })
            .unwrap();

            let report = recent_command_reports(1).unwrap().pop().unwrap();
            assert!(report.invocation_id.is_some());
            assert!(
                matches!(
                    report.evidence_kind.as_str(),
                    "unique-shape:metadata-only" | "unique-shape:invocation-preview"
                ),
                "{}",
                report.evidence_kind
            );
        },
    );
}

#[test]
fn reports_created_while_commands_run_receive_late_exact_failure_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let repo = home.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let home_text = home.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home_text.as_str())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_THREAD_ID", Some("late-evidence-thread")),
        ],
        || run_late_failure_evidence_contract(&home, &repo),
    );
}

fn run_late_failure_evidence_contract(home: &Path, repo: &Path) {
    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(repo).unwrap();
    set_insight_setting("record_invocations", "true").unwrap();
    set_insight_setting("record_command_text", "true").unwrap();
    set_insight_setting("record_command_shape", "true").unwrap();
    set_insight_setting("record_response_previews", "false").unwrap();
    set_insight_setting("record_failures", "true").unwrap();
    set_insight_setting("record_failure_responses", "true").unwrap();

    let report_id = record_pending_node_report();
    record_late_node_failure();
    assert_late_node_report_evidence(home, report_id);
    std::env::set_current_dir(original_cwd).unwrap();
}

fn record_pending_node_report() -> u64 {
    let command = "node tests/slow-check.mjs";
    let args = ["node", "tests/slow-check.mjs"].map(str::to_string);
    let shape = crate::support::redaction::argument_shape_join(&args);
    let receipt = record_command_report(&CommandReportRecord {
        command,
        command_family: "node run",
        command_shape: &shape,
        command_shape_hash: &crate::support::redaction::stable_shape_hash(&shape),
        issue_kind: "incorrect_output",
        note: "reported while the command was still running",
    })
    .unwrap();
    assert_eq!(
        recent_command_reports(1).unwrap()[0].evidence_kind,
        "no-match"
    );
    receipt.id
}

fn record_late_node_failure() {
    let artifact_ref = "~/.cx/cache/failures/passthrough/late-evidence.log";
    record_failure_artifact(&FailureArtifactRecord {
        display_path: artifact_ref,
        tool_name: "node",
        compression: "none",
        stdout_bytes: 0,
        stderr_bytes: 50,
        original_bytes: 50,
        stored_bytes: 50,
    })
    .unwrap();
    let raw_response = "native timeout detail\n";
    let cx_response = format!("native timeout detail\n[full output: {artifact_ref}]");
    let raw = OutputObservation::from_text("passthrough:node", raw_response);
    let failure = FailureDetailRecord {
        command_family: "node run",
        command_line: "cx -- node tests/slow-check.mjs",
        exit_code: 1,
        cx_response: &cx_response,
        raw_source: Some("passthrough:node"),
        raw_response: Some(raw_response),
    };
    record_invocation_with_context_and_failure(
        &InvocationRecord {
            command: "node run",
            exit_code: 1,
            raw: Some(&raw),
            emitted: TextMetrics::from_text(&cx_response),
        },
        Some(&InvocationContext {
            process: "node",
            command: "node tests/slow-check.mjs",
            argv_json: r#"["cx","--","node","tests/slow-check.mjs"]"#,
            emitted_response: Some(&cx_response),
        }),
        Some(&failure),
    )
    .unwrap();
}

fn assert_late_node_report_evidence(home: &Path, report_id: u64) {
    let artifact_ref = "~/.cx/cache/failures/passthrough/late-evidence.log";
    let raw_response = "native timeout detail\n";
    let report = recent_command_reports(1).unwrap().pop().unwrap();
    assert_eq!(report.id, report_id);
    assert!(report.invocation_id.is_some());
    assert_eq!(report.evidence_kind, "exact-command:late-failure-detail");
    assert!(report.cx_response.contains("native timeout detail"));
    assert_eq!(report.native_response, raw_response);
    assert_eq!(report.artifact_ref, artifact_ref);

    let connection = rusqlite::Connection::open(home.join(".cx/db.sqlite")).unwrap();
    let artifact_report_id = connection
        .query_row(
            "SELECT report_id FROM failure_artifacts WHERE display_path = ?1",
            [artifact_ref],
            |row| row.get::<_, u64>(0),
        )
        .unwrap();
    assert_eq!(artifact_report_id, report_id);
    let pending_at_ms = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'pending_report_evidence_at_ms'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(pending_at_ms, "0");
    assert!(!insight_settings_snapshot()
        .unwrap()
        .rows
        .iter()
        .any(|row| row.key == "pending_report_evidence_at_ms"));
}

#[test]
fn late_evidence_backfill_requires_the_exact_retained_command() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("reports.sqlite");
    let database_text = database.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(database_text.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            set_insight_setting("record_invocations", "true").unwrap();
            set_insight_setting("record_command_text", "true").unwrap();
            set_insight_setting("record_command_shape", "true").unwrap();
            let report_args = ["bash", "-lc", "alpha-script"].map(str::to_string);
            let shape = crate::support::redaction::argument_shape_join(&report_args);
            record_command_report(&CommandReportRecord {
                command: "bash -lc alpha-script",
                command_family: "sh",
                command_shape: &shape,
                command_shape_hash: &crate::support::redaction::stable_shape_hash(&shape),
                issue_kind: "incorrect_output",
                note: "must not borrow a later different command",
            })
            .unwrap();

            record_invocation_with_context(
                &InvocationRecord {
                    command: "sh",
                    exit_code: 0,
                    raw: None,
                    emitted: TextMetrics::from_text("done\n"),
                },
                Some(&InvocationContext {
                    process: "sh",
                    command: "bash -lc beta-script",
                    argv_json: r#"["cx","--","bash","-lc","beta-script"]"#,
                    emitted_response: Some("done\n"),
                }),
            )
            .unwrap();

            let report = recent_command_reports(1).unwrap().pop().unwrap();
            assert_eq!(report.invocation_id, None);
            assert_eq!(report.evidence_kind, "no-match");
        },
    );
}

fn run_report_evidence_contract(home: &Path, repo: &Path) {
    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(repo).unwrap();
    set_insight_setting("record_invocations", "true").unwrap();
    set_insight_setting("record_command_text", "true").unwrap();
    record_preview_disabled_invocation(&home.join(".cx/db.sqlite"));

    set_insight_setting("record_failures", "true").unwrap();
    set_insight_setting("record_failure_responses", "true").unwrap();
    record_linked_failure_invocation();
    record_unrelated_same_family_invocation();
    assert_report_and_artifact_evidence();
    std::env::set_current_dir(original_cwd).unwrap();
}

fn record_preview_disabled_invocation(database: &Path) {
    let first_raw = OutputObservation::from_text("native:rg", "native first response\n");
    record_invocation_with_context(
        &InvocationRecord {
            command: "grep",
            exit_code: 0,
            raw: Some(&first_raw),
            emitted: TextMetrics::from_text("cx first response\n"),
        },
        Some(&InvocationContext {
            process: "grep",
            command: "rg first src",
            argv_json: r#"["cx","--","rg","first","src"]"#,
            emitted_response: Some("cx first response\n"),
        }),
    )
    .unwrap();

    let connection = rusqlite::Connection::open(database).unwrap();
    let first_previews = connection
        .query_row(
            "SELECT emitted_response_preview, raw_response_preview
             FROM command_invocations ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(first_previews, (String::new(), String::new()));
}

fn record_linked_failure_invocation() {
    record_failure_artifact(&FailureArtifactRecord {
        display_path: EVIDENCE_ARTIFACT_REF,
        tool_name: "grep",
        compression: "gzip",
        stdout_bytes: 20_000,
        stderr_bytes: 1_000,
        original_bytes: 21_000,
        stored_bytes: 2_000,
    })
    .unwrap();

    let raw_response = format!("native regex response {EVIDENCE_SECRET}\n");
    let cx_response =
        format!("cx regex response {EVIDENCE_SECRET}\n[full output: {EVIDENCE_ARTIFACT_REF}]");
    let raw = OutputObservation::from_text("native:rg", &raw_response);
    let failure = FailureDetailRecord {
        command_family: "grep",
        command_line: "cx -- rg '(' src",
        exit_code: 2,
        cx_response: &cx_response,
        raw_source: Some("native:rg"),
        raw_response: Some(&raw_response),
    };
    record_invocation_with_context_and_failure(
        &InvocationRecord {
            command: "grep",
            exit_code: 2,
            raw: Some(&raw),
            emitted: TextMetrics::from_text(&cx_response),
        },
        Some(&InvocationContext {
            process: "grep",
            command: "rg '(' src",
            argv_json: r#"["cx","--","rg","(","src"]"#,
            emitted_response: Some(&cx_response),
        }),
        Some(&failure),
    )
    .unwrap();
}

fn record_unrelated_same_family_invocation() {
    let raw = OutputObservation::from_text("native:rg", "unrelated native response\n");
    record_invocation_with_context(
        &InvocationRecord {
            command: "grep",
            exit_code: 0,
            raw: Some(&raw),
            emitted: TextMetrics::from_text("unrelated cx response\n"),
        },
        Some(&InvocationContext {
            process: "grep",
            command: "rg unrelated src",
            argv_json: r#"["cx","--","rg","unrelated","src"]"#,
            emitted_response: Some("unrelated cx response\n"),
        }),
    )
    .unwrap();
}

fn assert_report_and_artifact_evidence() {
    let receipt = record_command_report(&CommandReportRecord {
        command: "rg '(' src",
        command_family: "grep",
        command_shape: "rg '<pattern>' '<value>'",
        command_shape_hash: &crate::support::redaction::stable_shape_hash(
            "rg '<pattern>' '<value>'",
        ),
        issue_kind: "incorrect_output",
        note: "regex dialect mismatch",
    })
    .unwrap();
    let report = recent_command_reports(1).unwrap().pop().unwrap();
    assert_eq!(report.id, receipt.id);
    assert!(report.invocation_id.is_some());
    assert_eq!(report.evidence_kind, "exact-command:failure-detail");
    assert_eq!(report.thread_id, "evidence-thread");
    assert_eq!(report.artifact_ref, EVIDENCE_ARTIFACT_REF);
    assert!(report.cx_response.contains("[REDACTED]"));
    assert!(report.native_response.contains("[REDACTED]"));
    assert!(!report.cx_response.contains(EVIDENCE_SECRET));
    assert!(!report.native_response.contains(EVIDENCE_SECRET));
    assert!(!report.binary_version.is_empty());
    assert!(!report.binary_revision.is_empty());
    assert!(!report.binary_fingerprint.is_empty());

    let failure = recent_failure_details(1).unwrap().pop().unwrap();
    assert_eq!(failure.invocation_id, report.invocation_id);
    assert!(failure.cx_response.contains("[REDACTED]"));
    assert!(failure.raw_response.contains("[REDACTED]"));

    let artifact = recent_failure_artifacts(1).unwrap().pop().unwrap();
    assert_eq!(artifact.display_path, EVIDENCE_ARTIFACT_REF);
    assert_eq!(artifact.invocation_id, report.invocation_id);
    assert_eq!(artifact.report_id, Some(report.id));
    assert_eq!(artifact.exit_code, 2);
    assert_eq!(artifact.original_bytes, 21_000);
    assert_eq!(artifact.stored_bytes, 2_000);
}

#[test]
fn opportunity_queries_filter_by_recent_window_and_confidence() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("opportunities.sqlite");
    let database_text = database.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(database_text.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            for (family, confidence) in [
                ("passthrough node", OpportunityConfidence::High),
                ("passthrough cmake", OpportunityConfidence::Medium),
                ("passthrough custom", OpportunityConfidence::Low),
            ] {
                record_command_opportunity(&CommandOpportunityRecord {
                    process: "passthrough",
                    command_family: family,
                    command: family,
                    source: family,
                    strategy: "test-projection",
                    confidence,
                    raw: TextMetrics::from_text("one\ntwo\nthree\nfour\n"),
                    projected: TextMetrics::from_text("one\nfour\n"),
                })
                .unwrap();
            }

            let medium = command_opportunities_filtered(
                10,
                OpportunityFilter {
                    since_ms: Some(0),
                    minimum_confidence: OpportunityConfidence::Medium,
                },
            )
            .unwrap();
            assert_eq!(medium.len(), 2);
            assert!(medium
                .iter()
                .all(|row| row.confidence != OpportunityConfidence::Low));

            let recent = command_opportunities_filtered(
                10,
                OpportunityFilter {
                    since_ms: Some(now_ms().saturating_add(60_000)),
                    minimum_confidence: OpportunityConfidence::Low,
                },
            )
            .unwrap();
            assert!(recent.is_empty());
        },
    );
}
