use std::env;
use std::ffi::OsString;
use std::sync::Mutex;

use cx::support::insights::{
    command_report_denial_reason_summary, recent_command_reports_filtered_by_status,
    record_command_report, triage_command_reports, update_command_report_disposition,
    CommandFilter, CommandReportDenialReason, CommandReportDispositionRecord, CommandReportRecord,
    CommandReportStatus, GENERIC_COMMAND_REPORT_NOTE,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn conservative_report_triage_is_dry_run_first_and_idempotent() {
    let _lock = ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let _env = EnvGuard::set(&[
        (
            "CX_INSIGHTS_DB_PATH",
            Some(db_path.to_string_lossy().as_ref()),
        ),
        ("CX_DISABLE_INSIGHTS", None),
    ]);
    let fixture = seed_triage_fixture(&db_path);
    assert_triage_preview(&db_path, fixture);
    assert_triage_apply(fixture);
    assert_triage_survivors(fixture);
    assert_triage_idempotence();
}

#[derive(Clone, Copy)]
struct TriageFixture {
    duplicate_old: u64,
    duplicate_response: u64,
    bare: u64,
    generic: u64,
    specific: u64,
    native_response: u64,
    paired_response: u64,
    invocation_only: u64,
    artifact_only: u64,
}

fn seed_triage_fixture(db_path: &std::path::Path) -> TriageFixture {
    let fixture = TriageFixture {
        duplicate_old: record("git diff", "git diff", GENERIC_COMMAND_REPORT_NOTE),
        duplicate_response: record("git diff", "git diff", GENERIC_COMMAND_REPORT_NOTE),
        bare: record("bash", "sh", GENERIC_COMMAND_REPORT_NOTE),
        generic: record("grep route", "grep", GENERIC_COMMAND_REPORT_NOTE),
        specific: record("find src", "find", "specific reproduction with exact paths"),
        native_response: record(
            "node --check app.js",
            "node check",
            GENERIC_COMMAND_REPORT_NOTE,
        ),
        paired_response: record(
            "cmake --build build",
            "cmake build",
            GENERIC_COMMAND_REPORT_NOTE,
        ),
        invocation_only: record(
            "node script.mjs --check",
            "node run",
            GENERIC_COMMAND_REPORT_NOTE,
        ),
        artifact_only: record("cargo test fee", "cargo test", GENERIC_COMMAND_REPORT_NOTE),
    };
    let terminal = record("git status", "git status", GENERIC_COMMAND_REPORT_NOTE);

    set_responses(
        db_path,
        fixture.duplicate_response,
        "cx response",
        "",
        "exact-command:failure-detail",
    );
    set_responses(
        db_path,
        fixture.native_response,
        "",
        "native response",
        "exact-command:failure-detail",
    );
    set_responses(
        db_path,
        fixture.paired_response,
        "cx response",
        "native response",
        "exact-command:failure-detail",
    );
    set_actionable_metadata(
        db_path,
        fixture.invocation_only,
        Some(42),
        "",
        "exact-command:metadata-only",
    );
    set_actionable_metadata(
        db_path,
        fixture.artifact_only,
        None,
        "~/.cx/cache/failures/cargo/fixture.log",
        "exact-command:failure-detail",
    );
    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: terminal,
        status: CommandReportStatus::Resolved,
        denial_reason: None,
        related_report_id: None,
        note: "already resolved",
        revision: "r126",
    })
    .unwrap();
    fixture
}

fn assert_triage_preview(db_path: &std::path::Path, fixture: TriageFixture) {
    let disposition_rows_before = disposition_count(db_path);
    let preview = triage_command_reports(false).unwrap();
    assert!(!preview.applied);
    assert_eq!(preview.denial_reasons.total, 3);
    assert_eq!(preview.denial_reasons.duplicate, 1);
    assert_eq!(preview.denial_reasons.invalid, 1);
    assert_eq!(preview.denial_reasons.insufficient_evidence, 1);
    assert_eq!(preview.current.total, 10);
    assert_eq!(preview.current.open, 9);
    assert_eq!(preview.projected.open, 6);
    assert_eq!(preview.projected.denied, 3);
    assert_eq!(disposition_count(db_path), disposition_rows_before);
    assert_eq!(
        preview
            .proposals
            .iter()
            .map(|proposal| (proposal.report_id, proposal.denial_reason))
            .collect::<Vec<_>>(),
        vec![
            (fixture.duplicate_old, CommandReportDenialReason::Duplicate),
            (fixture.bare, CommandReportDenialReason::Invalid),
            (
                fixture.generic,
                CommandReportDenialReason::InsufficientEvidence
            ),
        ]
    );
    assert_eq!(
        preview.proposals[0].related_report_id,
        Some(fixture.duplicate_response)
    );
}

fn assert_triage_apply(fixture: TriageFixture) {
    let applied = triage_command_reports(true).unwrap();
    assert!(applied.applied);
    assert_eq!(applied.applied_count, 3);
    assert_eq!(applied.skipped_count, 0);
    assert_eq!(applied.final_status.total, 10);
    assert_eq!(applied.final_status.open, 6);
    assert_eq!(applied.final_status.resolved, 1);
    assert_eq!(applied.final_status.denied, 3);

    let denied = recent_command_reports_filtered_by_status(
        10,
        CommandFilter::default(),
        Some(CommandReportStatus::Denied),
    )
    .unwrap();
    assert_eq!(denied.len(), 3);
    let duplicate = denied
        .iter()
        .find(|report| report.id == fixture.duplicate_old)
        .unwrap();
    assert_eq!(duplicate.denial_reason, "duplicate");
    assert_eq!(
        duplicate.related_report_id,
        Some(fixture.duplicate_response)
    );
}

fn assert_triage_survivors(fixture: TriageFixture) {
    let open_ids = recent_command_reports_filtered_by_status(
        20,
        CommandFilter::default(),
        Some(CommandReportStatus::Open),
    )
    .unwrap()
    .into_iter()
    .map(|report| report.id)
    .collect::<Vec<_>>();
    assert!(open_ids.contains(&fixture.duplicate_response));
    assert!(open_ids.contains(&fixture.specific));
    assert!(open_ids.contains(&fixture.native_response));
    assert!(open_ids.contains(&fixture.paired_response));
    assert!(open_ids.contains(&fixture.invocation_only));
    assert!(open_ids.contains(&fixture.artifact_only));

    let reasons = command_report_denial_reason_summary(CommandFilter::default()).unwrap();
    assert_eq!(reasons.total, 3);
    assert_eq!(reasons.duplicate, 1);
    assert_eq!(reasons.invalid, 1);
    assert_eq!(reasons.insufficient_evidence, 1);
}

fn assert_triage_idempotence() {
    let second_preview = triage_command_reports(false).unwrap();
    assert!(second_preview.proposals.is_empty());
    let second_apply = triage_command_reports(true).unwrap();
    assert_eq!(second_apply.applied_count, 0);
    assert!(second_apply.proposals.is_empty());
}

#[test]
fn denial_validation_and_reopen_clear_structured_metadata() {
    let _lock = ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let _env = EnvGuard::set(&[
        (
            "CX_INSIGHTS_DB_PATH",
            Some(db_path.to_string_lossy().as_ref()),
        ),
        ("CX_DISABLE_INSIGHTS", None),
    ]);
    let first = record("git diff", "git diff", "specific report");
    let second = record("git diff --stat", "git diff", "specific report");

    let missing_reason = update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: first,
        status: CommandReportStatus::Denied,
        denial_reason: None,
        related_report_id: None,
        note: "missing reason",
        revision: "r127",
    })
    .unwrap_err();
    assert!(missing_reason.to_string().contains("require --reason"));

    let missing_related = update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: first,
        status: CommandReportStatus::Denied,
        denial_reason: Some(CommandReportDenialReason::Duplicate),
        related_report_id: None,
        note: "missing related report",
        revision: "r127",
    })
    .unwrap_err();
    assert!(missing_related.to_string().contains("--related-report-id"));

    let self_related = update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: first,
        status: CommandReportStatus::Denied,
        denial_reason: Some(CommandReportDenialReason::Duplicate),
        related_report_id: Some(first),
        note: "self reference",
        revision: "r127",
    })
    .unwrap_err();
    assert!(self_related.to_string().contains("cannot reference itself"));

    let invalid_non_denied = update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: first,
        status: CommandReportStatus::Resolved,
        denial_reason: Some(CommandReportDenialReason::Obsolete),
        related_report_id: None,
        note: "wrong status",
        revision: "r127",
    })
    .unwrap_err();
    assert!(invalid_non_denied
        .to_string()
        .contains("valid only when report status is denied"));

    let denied = update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: first,
        status: CommandReportStatus::Denied,
        denial_reason: Some(CommandReportDenialReason::Duplicate),
        related_report_id: Some(second),
        note: "duplicate of the stronger report",
        revision: "r127",
    })
    .unwrap();
    assert_eq!(
        denied.denial_reason,
        Some(CommandReportDenialReason::Duplicate)
    );
    assert_eq!(denied.related_report_id, Some(second));

    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: first,
        status: CommandReportStatus::Open,
        denial_reason: None,
        related_report_id: None,
        note: "reopened with a current reproduction",
        revision: "",
    })
    .unwrap();
    let report = recent_command_reports_filtered_by_status(
        10,
        CommandFilter::default(),
        Some(CommandReportStatus::Open),
    )
    .unwrap()
    .into_iter()
    .find(|report| report.id == first)
    .unwrap();
    assert!(report.denial_reason.is_empty());
    assert_eq!(report.related_report_id, None);
}

#[test]
fn schema_18_disposition_constraint_migrates_to_19_without_losing_rows() {
    let _lock = ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("legacy.sqlite");
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT INTO schema_meta VALUES ('insights_schema_version', '18');
            CREATE TABLE command_reports (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                occurred_at_ms INTEGER NOT NULL,
                command TEXT NOT NULL,
                command_root TEXT NOT NULL DEFAULT '',
                command_family TEXT NOT NULL,
                issue_kind TEXT NOT NULL,
                note TEXT NOT NULL
            );
            INSERT INTO command_reports VALUES
                (1, 100, 'git diff', 'git', 'git diff', 'suspicious_output', 'specific report'),
                (2, 200, 'grep route', 'grep', 'grep', 'suspicious_output',
                 'Operator observed incorrect, misleading, incomplete, over-filtered, or suspicious CX output.');
            CREATE TABLE command_report_dispositions (
                report_id INTEGER PRIMARY KEY,
                status TEXT NOT NULL CHECK (
                    status IN ('open', 'resolved', 'native_parity', 'not_reproducible')
                ),
                note TEXT NOT NULL,
                revision TEXT NOT NULL DEFAULT '',
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO command_report_dispositions
                VALUES (1, 'resolved', 'legacy resolution', 'r126', 300);
            ",
        )
        .unwrap();
    drop(connection);
    let _env = EnvGuard::set(&[
        (
            "CX_INSIGHTS_DB_PATH",
            Some(db_path.to_string_lossy().as_ref()),
        ),
        ("CX_DISABLE_INSIGHTS", None),
    ]);

    let preview = triage_command_reports(false).unwrap();
    assert_eq!(preview.denial_reasons.insufficient_evidence, 1);
    assert_eq!(schema_version(&db_path), "18");

    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: 2,
        status: CommandReportStatus::Denied,
        denial_reason: Some(CommandReportDenialReason::Obsolete),
        related_report_id: None,
        note: "superseded by the current command contract",
        revision: "r127",
    })
    .unwrap();

    assert_migrated_disposition_schema(&db_path);
}

fn assert_migrated_disposition_schema(db_path: &std::path::Path) {
    assert_eq!(schema_version(db_path), "19");
    let connection = rusqlite::Connection::open(db_path).unwrap();
    let table_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'command_report_dispositions'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(table_sql.contains("'denied'"));
    let rows: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM command_report_dispositions",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 2);
    let first: (String, String, String) = connection
        .query_row(
            "SELECT status, note, revision
             FROM command_report_dispositions WHERE report_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        first,
        (
            "resolved".to_string(),
            "legacy resolution".to_string(),
            "r126".to_string()
        )
    );
    let second: (String, String, Option<u64>) = connection
        .query_row(
            "SELECT status, denial_reason, related_report_id
             FROM command_report_dispositions WHERE report_id = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(second, ("denied".to_string(), "obsolete".to_string(), None));
}

fn record(command: &str, command_family: &str, note: &str) -> u64 {
    record_command_report(&CommandReportRecord {
        command,
        command_family,
        command_shape: "",
        command_shape_hash: "",
        issue_kind: "suspicious_output",
        note,
    })
    .unwrap()
    .id
}

fn set_responses(
    db_path: &std::path::Path,
    report_id: u64,
    cx_response: &str,
    native_response: &str,
    evidence_kind: &str,
) {
    let connection = rusqlite::Connection::open(db_path).unwrap();
    connection
        .execute(
            "
            UPDATE command_report_evidence
            SET cx_response = ?1, native_response = ?2, evidence_kind = ?3
            WHERE report_id = ?4
            ",
            (cx_response, native_response, evidence_kind, report_id),
        )
        .unwrap();
}

fn set_actionable_metadata(
    db_path: &std::path::Path,
    report_id: u64,
    invocation_id: Option<u64>,
    artifact_ref: &str,
    evidence_kind: &str,
) {
    let connection = rusqlite::Connection::open(db_path).unwrap();
    connection
        .execute(
            "
            UPDATE command_report_evidence
            SET invocation_id = ?1, artifact_ref = ?2, evidence_kind = ?3
            WHERE report_id = ?4
            ",
            (invocation_id, artifact_ref, evidence_kind, report_id),
        )
        .unwrap();
}

fn disposition_count(db_path: &std::path::Path) -> u64 {
    rusqlite::Connection::open(db_path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM command_report_dispositions",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn schema_version(db_path: &std::path::Path) -> String {
    rusqlite::Connection::open(db_path)
        .unwrap()
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'insights_schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

struct EnvGuard {
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    fn set(entries: &[(&'static str, Option<&str>)]) -> Self {
        let previous = entries
            .iter()
            .map(|(key, _)| (*key, env::var_os(key)))
            .collect();
        for (key, value) in entries {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
        Self { previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }
}
