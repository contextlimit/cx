use super::*;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

#[test]
fn explicit_settings_wait_for_a_busy_telemetry_database() {
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
            let locker = rusqlite::Connection::open(&db_path).unwrap();
            locker.execute_batch("BEGIN EXCLUSIVE").unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let worker_barrier = Arc::clone(&barrier);
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                let started = Instant::now();
                let snapshot = set_insight_settings(&[
                    ("record_failures", "true"),
                    ("record_failure_responses", "true"),
                ])
                .unwrap();
                (snapshot, started.elapsed())
            });

            barrier.wait();
            std::thread::sleep(Duration::from_millis(250));
            locker.execute_batch("COMMIT").unwrap();
            let (snapshot, elapsed) = worker.join().unwrap();

            assert!(elapsed >= Duration::from_millis(200));
            assert!(snapshot
                .rows
                .iter()
                .any(|row| row.key == "record_failures" && row.value));
            assert!(snapshot
                .rows
                .iter()
                .any(|row| row.key == "record_failure_responses" && row.value));
        },
    );
}

#[test]
fn isolated_database_keeps_all_enabled_baseline_but_respects_explicit_overrides() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let initial = settings::recording_settings().unwrap();
            assert!(initial.record_invocations);
            assert!(initial.record_command_text);
            assert!(initial.record_response_previews);
            let initial_snapshot = insight_settings_snapshot().unwrap();
            assert!(initial_snapshot
                .rows
                .iter()
                .any(|row| row.key == "record_response_previews" && row.value));

            set_insight_settings(&[
                ("record_command_text", "false"),
                ("record_response_previews", "false"),
            ])
            .unwrap();

            let explicit = settings::recording_settings().unwrap();
            assert!(explicit.record_invocations);
            assert!(!explicit.record_command_text);
            assert!(!explicit.record_response_previews);
        },
    );
}

#[test]
fn settings_enable_metrics_without_command_text_or_sources() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_THREAD_ID", Some("thread-local-context")),
            ("CX_PLAN_TITLE", Some("Local Context Plan")),
            ("CX_PLAN_PROJECT_ID", Some("sample:plan:localctx123")),
            ("CX_PLAN_FOLDER_ID", Some("localctx123")),
        ],
        || {
            std::fs::create_dir_all(home.join("repo/.git")).unwrap();
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(home.join("repo")).unwrap();
            set_insight_setting("record_invocations", "true").unwrap();
            let raw = OutputObservation::from_text("secret/path.txt", "a\nb\n");
            record_invocation_with_context(
                &InvocationRecord {
                    command: "read",
                    exit_code: 0,
                    raw: Some(&raw),
                    emitted: TextMetrics::from_text("a\n"),
                },
                Some(&InvocationContext {
                    process: "read",
                    command: "read secret/path.txt",
                    argv_json: r#"["cx","read","secret/path.txt"]"#,
                    emitted_response: Some("a\n"),
                }),
            )
            .unwrap();
            std::env::set_current_dir(original).unwrap();

            let db_path = home.join(".cx/db.sqlite");
            let connection = rusqlite::Connection::open(db_path).unwrap();
            let repo_path = std::fs::canonicalize(home.join("repo")).unwrap();
            let repo_path = repo_path.to_string_lossy().to_string();
            let row = connection
                .query_row(
                    "SELECT process, command_family, command, source, argv_json,
                            command_shape, command_shape_hash,
                            thread_id, plan_title, plan_project_id, plan_folder_id, cwd, git_root
                     FROM command_invocations",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, String>(9)?,
                            row.get::<_, String>(10)?,
                            row.get::<_, String>(11)?,
                            row.get::<_, String>(12)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(row.0, "read");
            assert_eq!(row.1, "read");
            assert_eq!(row.2, "read");
            assert_eq!(row.3, "not recorded");
            assert_eq!(row.4, "[]");
            assert_eq!(row.5, "read '<path:txt>'");
            assert!(!row.6.is_empty());
            assert_eq!(row.7, "thread-local-context");
            assert_eq!(row.8, "Local Context Plan");
            assert_eq!(row.9, "sample:plan:localctx123");
            assert_eq!(row.10, "localctx123");
            assert_eq!(row.11, repo_path);
            assert_eq!(row.12, repo_path);
        },
    );
}

#[test]
fn command_shape_strips_cx_auto_mode_marker_without_recording_argv_text() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            set_insight_setting("record_invocations", "true").unwrap();
            let raw = OutputObservation::from_text("passthrough:sed", "one\ntwo\n");
            record_invocation_with_context(
                &InvocationRecord {
                    command: "passthrough sed",
                    exit_code: 0,
                    raw: Some(&raw),
                    emitted: TextMetrics::from_text("one\n"),
                },
                Some(&InvocationContext {
                    process: "passthrough",
                    command: "sed -n '1,5p' file.txt",
                    argv_json: r#"["cx","--","sed","-n","1,5p","file.txt"]"#,
                    emitted_response: Some("one\n"),
                }),
            )
            .unwrap();

            let connection = rusqlite::Connection::open(home.join(".cx/db.sqlite")).unwrap();
            let row = connection
                .query_row(
                    "SELECT command, argv_json, command_shape, command_shape_hash
                     FROM command_invocations",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(row.0, "passthrough sed");
            assert_eq!(row.1, "[]");
            assert_eq!(row.2, "sed -n '<value>' '<path:txt>'");
            assert!(!row.3.is_empty());
        },
    );
}

#[test]
fn settings_can_disable_and_reenable_default_passthrough() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
        ],
        || {
            assert!(unsupported_passthrough_enabled().unwrap());
            assert!(!home.join(".cx").exists());
            set_insight_setting("passthrough_unsupported_commands", "false").unwrap();
            assert!(!unsupported_passthrough_enabled().unwrap());
            set_insight_setting("passthrough_unsupported_commands", "true").unwrap();
            assert!(unsupported_passthrough_enabled().unwrap());
        },
    );
}

#[test]
fn default_passthrough_survives_disabled_insights() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", Some("1")),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
        ],
        || {
            assert!(unsupported_passthrough_enabled().unwrap());
            assert!(!home.join(".cx").exists());
        },
    );
}

#[test]
fn legacy_default_rows_adopt_current_defaults_but_explicit_rows_do_not() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            create_schema(&connection).unwrap();
            connection
                .execute(
                    "UPDATE settings SET value = 'false', updated_at_ms = 0 \
                     WHERE key IN ('record_invocations', 'passthrough_unsupported_commands')",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE schema_meta SET value = '19' \
                     WHERE key = 'insights_schema_version'",
                    [],
                )
                .unwrap();
            drop(connection);

            let defaults = settings::recording_settings().unwrap();
            assert!(defaults.record_invocations);
            assert!(defaults.passthrough_unsupported_commands);

            set_insight_setting("passthrough_unsupported_commands", "false").unwrap();
            let explicit = settings::recording_settings().unwrap();
            assert!(explicit.record_invocations);
            assert!(!explicit.passthrough_unsupported_commands);

            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let rows = connection
                .query_row(
                    "SELECT \
                        (SELECT value FROM settings WHERE key = 'record_invocations'), \
                        (SELECT value FROM settings WHERE key = 'passthrough_unsupported_commands')",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .unwrap();
            assert_eq!(rows, ("true".to_string(), "false".to_string()));
        },
    );
}

#[test]
fn compact_document_search_results_defaults_false_and_can_be_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            assert!(!compact_document_search_results_enabled().unwrap());
            assert!(!home.join(".cx/db.sqlite").exists());
            set_insight_setting("compact_document_search_results", "true").unwrap();
            assert!(compact_document_search_results_enabled().unwrap());
        },
    );
}

#[test]
fn command_opportunities_group_potential_savings_by_family() {
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
            let raw = TextMetrics::from_text(&"line\n".repeat(80));
            let projected = TextMetrics::from_text("line\n... [40 lines omitted] ...\nline\n");
            let recorded = record_command_opportunity(&CommandOpportunityRecord {
                process: "passthrough",
                command_family: "passthrough seq",
                command: "seq 1 80",
                source: "passthrough:seq",
                strategy: "test-generic",
                confidence: OpportunityConfidence::Low,
                raw,
                projected,
            })
            .unwrap();
            assert!(recorded);

            let opportunities = command_opportunities(10).unwrap();
            assert_eq!(opportunities.len(), 1);
            assert_eq!(opportunities[0].process, "passthrough");
            assert_eq!(opportunities[0].command_family, "passthrough seq");
            assert_eq!(opportunities[0].samples, 1);
            assert_eq!(opportunities[0].strategy, "test-generic");
            assert!(opportunities[0].potential_saved.lines > 0);
            assert!(opportunities[0].potential_saved.tokens > 0);
        },
    );
}

#[test]
fn command_repair_records_rule_action_and_exit_transition() {
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
            let recorded = record_command_repair(&CommandRepairRecord {
                process: "passthrough",
                command_family: "passthrough jq",
                command: "jq -r '<filter>' ledger.json",
                source: "passthrough:jq",
                rule_id: "jq.comma_pipe_precedence",
                action: "auto_retry_success",
                original_exit_code: 5,
                final_exit_code: 0,
                original_response: "jq: error: Cannot iterate over number (10)",
                final_response: "10\nOB-02\n",
            })
            .unwrap();
            assert!(recorded);

            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let row = connection
                .query_row(
                    "SELECT process, command_family, rule_id, action, original_exit_code, final_exit_code, original_response, final_response FROM command_repairs",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i32>(4)?,
                            row.get::<_, i32>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    },
                )
                .unwrap();

            assert_eq!(row.0, "passthrough");
            assert_eq!(row.1, "passthrough jq");
            assert_eq!(row.2, "jq.comma_pipe_precedence");
            assert_eq!(row.3, "auto_retry_success");
            assert_eq!(row.4, 5);
            assert_eq!(row.5, 0);
            assert!(row.6.contains("Cannot iterate"));
            assert!(row.7.contains("OB-02"));
        },
    );
}
