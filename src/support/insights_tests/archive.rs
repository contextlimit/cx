use super::*;

#[test]
fn archive_summary_uses_canonical_dedupe_and_reports_quality_gaps() {
    let temp = tempfile::tempdir().unwrap();
    let archive_a = temp.path().join("project-a-archive.sqlite");
    let archive_b = temp.path().join("project-b-archive.sqlite");
    seed_archive_database(&archive_a, true);
    seed_archive_database(&archive_b, false);

    let summary = archive_summary(&[archive_a, archive_b], 10).unwrap();
    assert_archive_dedupe(&summary);
    assert_archive_totals(&summary);
    assert_archive_command_rankings(&summary);
    assert_archive_coverage(&summary);
}

fn assert_archive_dedupe(summary: &ArchiveSummary) {
    assert_eq!(summary.dedupe.raw_archive_rows, 9);
    assert_eq!(summary.dedupe.invocation_id_rows, 8);
    assert_eq!(summary.dedupe.canonical_rows, 7);
    assert_eq!(summary.dedupe.archive_duplicate_rows, 1);
    assert_eq!(summary.dedupe.import_fanout_rows, 1);
}

fn assert_archive_totals(summary: &ArchiveSummary) {
    assert_eq!(summary.overall.invocations, 7);
    assert_eq!(summary.overall.raw.tokens, 212);
    assert_eq!(summary.overall.emitted.tokens, 106);
    assert_eq!(summary.overall.saved.tokens, 105);
    assert_eq!(summary.overall.empty_argv, 1);

    let official = summary
        .support_classes
        .iter()
        .find(|row| row.support_class == "official")
        .unwrap();
    assert_eq!(official.invocations, 6);
    assert_eq!(official.raw_tokens, 172);
    assert_eq!(official.saved_tokens, 105);
}

fn assert_archive_command_rankings(summary: &ArchiveSummary) {
    assert_eq!(summary.top_saved_commands[0].command_family, "grep");
    assert_eq!(
        summary.top_opportunity_commands[0].command_family,
        "passthrough sed"
    );
    assert!(!summary
        .top_opportunity_commands
        .iter()
        .any(|row| row.command_family == "read"));
    assert_eq!(summary.expansion[0].command_family, "node check");
    assert!(!summary
        .expansion
        .iter()
        .any(|row| row.command_family == "mixed expansion"));
}

fn assert_archive_coverage(summary: &ArchiveSummary) {
    let sed_quality = summary
        .command_quality
        .iter()
        .find(|row| row.command_family == "passthrough sed")
        .unwrap();
    assert_eq!(sed_quality.empty_argv, 1);
    assert_eq!(sed_quality.family_only_command, 1);
    assert_eq!(sed_quality.empty_shape, 0);
    assert_eq!(sed_quality.family_only_shape, 0);

    assert_eq!(summary.failure_coverage.rows, 1);
    assert_eq!(summary.failure_coverage.artifact_rows, 1);
    assert_eq!(summary.failure_coverage.preview_rows, 1);
    assert_eq!(summary.report_coverage.rows, 1);
    assert_eq!(summary.report_coverage.preview_rows, 1);
}

struct ArchiveInvocationFixture<'a> {
    invocation_id: &'a str,
    occurred_at_ms: u64,
    source_machine: &'a str,
    command_family: &'a str,
    command: &'a str,
    argv_json: &'a str,
    metrics_json: String,
    metadata_json: &'a str,
}

fn seed_archive_database(path: &std::path::Path, primary: bool) {
    let connection = rusqlite::Connection::open(path).unwrap();
    create_archive_schema(&connection);
    insert_archive_invocation(
        &connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:1",
            occurred_at_ms: 1_000,
            source_machine: if primary {
                "build-host-a"
            } else {
                "build-host-b"
            },
            command_family: "grep",
            command: "grep",
            argv_json: r#"["cx","grep","needle","."]"#,
            metrics_json: archive_metrics(100, 5, 95),
            metadata_json: r#"{"process":"grep","sourceId":1,"contextSource":"sender_fallback","source":"cx_insights_remote_sender"}"#,
        },
    );
    if primary {
        seed_primary_archive_rows(&connection);
        seed_archive_coverage_rows(&connection);
    }
}

fn create_archive_schema(connection: &rusqlite::Connection) {
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

fn seed_primary_archive_rows(connection: &rusqlite::Connection) {
    insert_archive_invocation(
        connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-b:1",
            occurred_at_ms: 1_000,
            source_machine: "build-host-b",
            command_family: "grep",
            command: "grep",
            argv_json: "[]",
            metrics_json: archive_metrics(100, 5, 95),
            metadata_json: r#"{"process":"grep","sourceId":1,"contextSource":"import_fallback","source":"cx_insights"}"#,
        },
    );
    insert_archive_invocation(
        connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:2",
            occurred_at_ms: 2_000,
            source_machine: "build-host-a",
            command_family: "passthrough sed",
            command: "passthrough sed",
            argv_json: "[]",
            metrics_json: archive_metrics(40, 40, 0),
            metadata_json: r#"{"process":"passthrough","sourceId":2,"contextSource":"cx_row","source":"cx_insights","commandShape":"sed -n '<value>' '<path:txt>'","commandShapeHash":"shape-fixture-hash"}"#,
        },
    );
    insert_archive_invocation(
        connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:3",
            occurred_at_ms: 3_000,
            source_machine: "build-host-a",
            command_family: "node check",
            command: "node check",
            argv_json: r#"["cx","node","--check","ok.js"]"#,
            metrics_json: archive_metrics(1, 10, 0),
            metadata_json: r#"{"process":"node","sourceId":3,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_archive_invocation(
        connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:4",
            occurred_at_ms: 4_000,
            source_machine: "build-host-a",
            command_family: "read",
            command: "read --range 1:30 file.txt",
            argv_json: r#"["cx","read","--range","1:30","file.txt"]"#,
            metrics_json: archive_metrics(30, 30, 0),
            metadata_json: r#"{"process":"read","sourceId":4,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_archive_invocation(
        connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:5",
            occurred_at_ms: 5_000,
            source_machine: "build-host-a",
            command_family: "read",
            command: "read --range 1:20 file.txt",
            argv_json: r#"["cx","read","--range","1:20","file.txt"]"#,
            metrics_json: archive_metrics(20, 10, 10),
            metadata_json: r#"{"process":"read","sourceId":5,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_archive_invocation(
        connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:6",
            occurred_at_ms: 6_000,
            source_machine: "build-host-a",
            command_family: "mixed expansion",
            command: "mixed expansion sample",
            argv_json: r#"["cx","mixed","expansion"]"#,
            metrics_json: archive_metrics(1, 10, 0),
            metadata_json: r#"{"process":"mixed","sourceId":6,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
    insert_archive_invocation(
        connection,
        ArchiveInvocationFixture {
            invocation_id: "cx:build-host-a:7",
            occurred_at_ms: 7_000,
            source_machine: "build-host-a",
            command_family: "mixed expansion",
            command: "mixed expansion sample",
            argv_json: r#"["cx","mixed","expansion"]"#,
            metrics_json: archive_metrics(20, 1, 0),
            metadata_json: r#"{"process":"mixed","sourceId":7,"contextSource":"cx_row","source":"cx_insights"}"#,
        },
    );
}

fn seed_archive_coverage_rows(connection: &rusqlite::Connection) {
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

fn insert_archive_invocation(
    connection: &rusqlite::Connection,
    fixture: ArchiveInvocationFixture<'_>,
) {
    connection
        .execute(
            "INSERT INTO command_invocations
             (invocation_id, occurred_at_ms, source_machine, cwd, git_root, command_family,
              command, argv_json, command_shape, command_shape_hash, exit_code, metrics_json, metadata_json, thread_id, plan_title,
              plan_project_id, plan_folder_id)
             VALUES (?1, ?2, ?3, '/repo', '/repo', ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, '', '', '', '')",
            rusqlite::params![
                fixture.invocation_id,
                fixture.occurred_at_ms,
                fixture.source_machine,
                fixture.command_family,
                fixture.command,
                fixture.argv_json,
                archive_fixture_command_shape(fixture.command_family),
                archive_fixture_command_shape_hash(fixture.command_family),
                fixture.metrics_json,
                fixture.metadata_json,
            ],
        )
        .unwrap();
}

fn archive_fixture_command_shape(command_family: &str) -> &'static str {
    match command_family {
        "read" => "read --range '<range>' '<path:txt>'",
        "node check" => "node --check '<path:js>'",
        "grep" => "grep '<value>' '<path>'",
        _ => "",
    }
}

fn archive_fixture_command_shape_hash(command_family: &str) -> &'static str {
    if archive_fixture_command_shape(command_family).is_empty() {
        ""
    } else {
        "shape-fixture-hash"
    }
}

fn archive_metrics(raw_tokens: u64, emitted_tokens: u64, saved_tokens: u64) -> String {
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
