use super::*;
use crate::support::insights::OpportunityConfidence;

#[test]
fn format_count_adds_group_separators() {
    assert_eq!(format_count(0), "0");
    assert_eq!(format_count(999), "999");
    assert_eq!(format_count(1_234_567), "1,234,567");
}

#[test]
fn format_command_totals_renders_saved_columns() {
    let rendered = format_command_totals(&[CommandTotalInsight {
        command: "git diff".to_string(),
        invocations: 2,
        failures: 0,
        expansions: 0,
        raw: TextMetrics {
            chars: 1200,
            lines: 30,
            tokens: 300,
            bytes: 1200,
        },
        emitted: TextMetrics::default(),
        saved: TextMetrics {
            chars: 1200,
            lines: 30,
            tokens: 300,
            bytes: 1200,
        },
        expanded: TextMetrics::default(),
        best_saved_chars: 900,
        best_saved_tokens: 250,
        best_expanded_tokens: 0,
    }]);
    assert!(rendered.contains("git diff | 2 | 0 | 0 | 300 | 0 | -300 | 30 | 1,200"));
}

#[test]
fn run_settings_lists_defaults_without_creating_database_and_can_set_values() {
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
            let output = run_settings(&[]).unwrap();
            assert!(output.stdout.contains("cx insights: settings"));
            assert!(output.stdout.contains("Database exists: no"));
            assert!(output.stdout.contains("record_invocations | false"));
            assert!(output.stdout.contains("record_command_shape | true"));
            assert!(output.stdout.contains("command_optimizations | true"));
            assert!(output
                .stdout
                .contains("compact_document_search_results | false"));
            assert!(output
                .stdout
                .contains("passthrough_unsupported_commands | false"));
            assert!(!home.join(".cx/db.sqlite").exists());

            let output = run_settings(&["record_invocations=true".to_string()]).unwrap();
            assert!(output.stdout.contains("Database exists: yes"));
            assert!(output.stdout.contains("record_invocations | true"));
            assert!(output.stdout.contains("record_command_shape | true"));
            assert!(output.stdout.contains("command_optimizations | true"));
            assert!(output
                .stdout
                .contains("compact_document_search_results | false"));
            assert!(output
                .stdout
                .contains("passthrough_unsupported_commands | false"));
            assert!(home.join(".cx/db.sqlite").exists());

            let output = run_settings(&[
                "record_failures=true".to_string(),
                "record_failure_responses=true".to_string(),
            ])
            .unwrap();
            assert!(output.stdout.contains("record_failures | true"));
            assert!(output.stdout.contains("record_failure_responses | true"));

            let output =
                run_settings(&["passthrough_unsupported_commands=true".to_string()]).unwrap();
            assert!(output
                .stdout
                .contains("passthrough_unsupported_commands | true"));
            let output =
                run_settings(&["compact_document_search_results=true".to_string()]).unwrap();
            assert!(output
                .stdout
                .contains("compact_document_search_results | true"));
        },
    );
}

#[test]
fn run_settings_validates_every_assignment_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("insights.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let error = run_settings(&[
                "record_failures=true".to_string(),
                "unknown_setting=true".to_string(),
            ])
            .unwrap_err();
            assert!(error.to_string().contains("unknown insights setting"));
            assert!(!db_path.exists());
        },
    );
}

#[test]
fn run_opportunities_renders_potential_savings() {
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
            record_command_opportunity(&CommandOpportunityRecord {
                process: "passthrough",
                command_family: "passthrough seq",
                command: "seq 1 200",
                source: "passthrough:seq",
                strategy: "test-generic",
                confidence: OpportunityConfidence::Low,
                raw: TextMetrics::from_text(&"row\n".repeat(200)),
                projected: TextMetrics::from_text("row\n... [160 lines omitted] ...\nrow\n"),
            })
            .unwrap();

            let output = run_opportunities(5).unwrap();
            assert!(output
                .stdout
                .contains("cx insights: passthrough opportunities"));
            assert!(output.stdout.contains("passthrough seq"));
            assert!(output.stdout.contains("test-generic"));
            assert!(output.stdout.contains("potential saved tokens"));
        },
    );
}

#[test]
fn run_archive_summary_renders_canonical_archive_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let archive_a = temp.path().join("project-a.sqlite");
    let archive_b = temp.path().join("project-b.sqlite");
    seed_command_archive(&archive_a, true);
    seed_command_archive(&archive_b, false);

    let output = run_archive_summary(&[archive_a, archive_b], 5).unwrap();
    assert!(output.stdout.contains("cx insights: archive summary"));
    assert!(output.stdout.contains("Canonical source-command rows: 7"));
    assert!(output.stdout.contains("Import fanout rows: 1"));
    assert!(output
        .stdout
        .contains("Saved: 1,050 bytes, 105 estimated tokens"));
    assert!(output.stdout.contains("official | 6 | 172"));
    assert!(output.stdout.contains("passthrough | 1 | 40"));
    assert!(output.stdout.contains("grep | grep | 1 | 0 | 100 | 5 | 95"));
    assert!(output
        .stdout
        .contains("passthrough sed | passthrough | 1 | 0 | 40 | 40 | 0"));
    let opportunity_section = output
        .stdout
        .split("Top zero-savings opportunities:")
        .nth(1)
        .unwrap()
        .split("Source machines:")
        .next()
        .unwrap();
    assert!(!opportunity_section.contains("read | read"));
    let expansion_section = output
        .stdout
        .split("Expansion rows:")
        .nth(1)
        .unwrap()
        .split("Failure/report coverage:")
        .next()
        .unwrap();
    assert!(!expansion_section.contains("mixed expansion"));
    assert!(output.stdout.contains("empty shape %"));
    assert!(output
        .stdout
        .contains("passthrough sed | 1 | 1 | 100.00% | 1 | 100.00% | 0 | 0.00% | 0 | 0.00%"));
    assert!(output.stdout.contains("Failures: 1 rows, 1 distinct"));
    assert!(output
        .stdout
        .contains("Reports: 1 rows, 1 distinct, 1 with text"));
}

struct CommandArchiveInvocationFixture<'a> {
    invocation_id: &'a str,
    occurred_at_ms: u64,
    command_family: &'a str,
    command: &'a str,
    argv_json: &'a str,
    metrics_json: String,
    metadata_json: &'a str,
}

fn seed_command_archive(path: &Path, primary: bool) {
    let connection = rusqlite::Connection::open(path).unwrap();
    create_command_archive_schema(&connection);
    insert_command_archive_invocation(
        &connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:1",
            occurred_at_ms: 1_000,
            command_family: "grep",
            command: "grep",
            argv_json: r#"["cx","grep","needle","."]"#,
            metrics_json: command_archive_metrics(100, 5, 95),
            metadata_json: r#"{"process":"grep","sourceId":1,"contextSource":"sender_fallback","source":"cx_insights_remote_sender"}"#,
        },
    );
    if primary {
        seed_primary_command_archive_rows(&connection);
        seed_command_archive_coverage_rows(&connection);
    }
}

fn create_command_archive_schema(connection: &rusqlite::Connection) {
    connection
        .execute_batch(
            "
            CREATE TABLE command_invocations (
                invocation_id TEXT PRIMARY KEY,
                occurred_at_ms INTEGER NOT NULL,
                source_machine TEXT NOT NULL,
                cwd TEXT NOT NULL,
                git_root TEXT NOT NULL,
                command_family TEXT NOT NULL,
                command TEXT NOT NULL,
                argv_json TEXT NOT NULL,
                command_shape TEXT NOT NULL,
                command_shape_hash TEXT NOT NULL,
                exit_code INTEGER NOT NULL,
                metrics_json TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                thread_id TEXT,
                plan_title TEXT,
                plan_project_id TEXT,
                plan_folder_id TEXT
            );
            CREATE TABLE command_failures (
                failure_id TEXT PRIMARY KEY,
                invocation_id TEXT,
                occurred_at_ms INTEGER NOT NULL,
                command_family TEXT NOT NULL,
                command_line TEXT NOT NULL,
                exit_code INTEGER NOT NULL,
                response_preview TEXT NOT NULL,
                artifact_ref TEXT NOT NULL,
                metadata_json TEXT NOT NULL
            );
            CREATE TABLE command_reports (
                report_id TEXT PRIMARY KEY,
                invocation_id TEXT,
                occurred_at_ms INTEGER NOT NULL,
                command_family TEXT NOT NULL,
                report_text TEXT NOT NULL,
                metadata_json TEXT NOT NULL
            );
            ",
        )
        .unwrap();
}

fn seed_primary_command_archive_rows(connection: &rusqlite::Connection) {
    insert_command_archive_invocation(
        connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-b:1",
            occurred_at_ms: 1_000,
            command_family: "grep",
            command: "grep",
            argv_json: "[]",
            metrics_json: command_archive_metrics(100, 5, 95),
            metadata_json: r#"{"process":"grep","sourceId":1,"contextSource":"import_fallback","source":"cx_insights"}"#,
        },
    );
    insert_command_archive_invocation(
        connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:2",
            occurred_at_ms: 2_000,
            command_family: "passthrough sed",
            command: "passthrough sed",
            argv_json: "[]",
            metrics_json: command_archive_metrics(40, 40, 0),
            metadata_json: r#"{"process":"passthrough","sourceId":2,"contextSource":"cx_row","source":"cx_insights","commandShape":"sed -n '<value>' '<path:txt>'","commandShapeHash":"shape-fixture-hash"}"#,
        },
    );
    insert_command_archive_invocation(
        connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:3",
            occurred_at_ms: 3_000,
            command_family: "node check",
            command: "node check",
            argv_json: r#"["cx","node","--check","ok.js"]"#,
            metrics_json: command_archive_metrics(1, 10, 0),
            metadata_json: r#"{"process":"node","sourceId":3,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_command_archive_invocation(
        connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:4",
            occurred_at_ms: 4_000,
            command_family: "read",
            command: "read --range 1:30 file.txt",
            argv_json: r#"["cx","read","--range","1:30","file.txt"]"#,
            metrics_json: command_archive_metrics(30, 30, 0),
            metadata_json: r#"{"process":"read","sourceId":4,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_command_archive_invocation(
        connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:5",
            occurred_at_ms: 5_000,
            command_family: "read",
            command: "read --range 1:20 file.txt",
            argv_json: r#"["cx","read","--range","1:20","file.txt"]"#,
            metrics_json: command_archive_metrics(20, 10, 10),
            metadata_json: r#"{"process":"read","sourceId":5,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_command_archive_invocation(
        connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:6",
            occurred_at_ms: 6_000,
            command_family: "mixed expansion",
            command: "mixed expansion sample",
            argv_json: r#"["cx","mixed","expansion"]"#,
            metrics_json: command_archive_metrics(1, 10, 0),
            metadata_json: r#"{"process":"mixed","sourceId":6,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_command_archive_invocation(
        connection,
        CommandArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:7",
            occurred_at_ms: 7_000,
            command_family: "mixed expansion",
            command: "mixed expansion sample",
            argv_json: r#"["cx","mixed","expansion"]"#,
            metrics_json: command_archive_metrics(20, 1, 0),
            metadata_json: r#"{"process":"mixed","sourceId":7,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
}

fn seed_command_archive_coverage_rows(connection: &rusqlite::Connection) {
    connection
        .execute(
            "INSERT INTO command_failures VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                "failure-1",
                "cx:build-host-a:3",
                3_000,
                "node check",
                "cx node --check bad.js",
                1,
                "syntax error",
                "~/.cx/cache/failures/node/1.log",
                "{}",
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO command_reports VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "report-1",
                "cx:build-host-a:1",
                4_000,
                "grep",
                "broad regex returned no matches",
                "{}",
            ],
        )
        .unwrap();
}

fn insert_command_archive_invocation(
    connection: &rusqlite::Connection,
    fixture: CommandArchiveInvocationFixture<'_>,
) {
    connection
        .execute(
            "INSERT INTO command_invocations
             (invocation_id, occurred_at_ms, source_machine, cwd, git_root, command_family,
              command, argv_json, command_shape, command_shape_hash, exit_code, metrics_json, metadata_json, thread_id, plan_title,
              plan_project_id, plan_folder_id)
             VALUES (?1, ?2, 'build-host-a', '/repo', '/repo', ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, '', '', '', '')",
            rusqlite::params![
                fixture.invocation_id,
                fixture.occurred_at_ms,
                fixture.command_family,
                fixture.command,
                fixture.argv_json,
                command_archive_fixture_shape(fixture.command_family),
                command_archive_fixture_shape_hash(fixture.command_family),
                fixture.metrics_json,
                fixture.metadata_json,
            ],
        )
        .unwrap();
}

fn command_archive_fixture_shape(command_family: &str) -> &'static str {
    match command_family {
        "read" => "read --range '<range>' '<path:txt>'",
        "node check" => "node --check '<path:js>'",
        "grep" => "grep '<value>' '<path>'",
        _ => "",
    }
}

fn command_archive_fixture_shape_hash(command_family: &str) -> &'static str {
    if command_archive_fixture_shape(command_family).is_empty() {
        ""
    } else {
        "shape-fixture-hash"
    }
}

fn command_archive_metrics(raw_tokens: u64, emitted_tokens: u64, saved_tokens: u64) -> String {
    format!(
        r#"{{"rawBytes":{raw_bytes},"rawChars":{raw_bytes},"rawLines":{raw_lines},"rawTokens":{raw_tokens},"emittedBytes":{emitted_bytes},"emittedChars":{emitted_bytes},"emittedLines":{emitted_lines},"emittedTokens":{emitted_tokens},"savedBytes":{saved_bytes},"savedChars":{saved_bytes},"savedLines":{saved_lines},"savedTokens":{saved_tokens}}}"#,
        raw_bytes = raw_tokens * 10,
        emitted_bytes = emitted_tokens * 10,
        saved_bytes = saved_tokens * 10,
        raw_lines = raw_tokens / 10,
        emitted_lines = emitted_tokens / 5,
        saved_lines = saved_tokens / 10,
    )
}
