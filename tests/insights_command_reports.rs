use std::env;
use std::ffi::OsString;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use cx::support::insights::{
    command_report_status_summary, command_report_totals_at_level,
    command_report_totals_at_level_by_status, recent_command_reports_filtered,
    recent_command_reports_filtered_by_status, record_command_report,
    update_command_report_disposition, CommandFilter, CommandLevel, CommandReportDispositionRecord,
    CommandReportRecord, CommandReportStatus,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn command_report_queries_can_group_and_filter_by_root_or_command() {
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

    let report_ids = seed_report_fixture();
    install_schema_refresh_guard(&db_path);
    classify_report_fixture(report_ids);
    assert_report_grouping();
    assert_report_status_filters();
    assert_report_reopen_and_missing(report_ids[0]);
    assert_concurrent_writer_retry(&db_path, report_ids[0]);
}

fn install_schema_refresh_guard(db_path: &std::path::Path) {
    let connection = rusqlite::Connection::open(db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TRIGGER reject_redundant_schema_refresh
            BEFORE INSERT ON schema_meta
            WHEN NEW.key = 'insights_schema_version'
            BEGIN
                SELECT RAISE(ABORT, 'report disposition should not refresh the full schema');
            END;
            ",
        )
        .unwrap();
}

fn seed_report_fixture() -> [u64; 3] {
    let mut ids = [0; 3];
    for (index, (command, command_family)) in [
        ("cx git diff -- src", "git diff"),
        ("cx git status --short", "git status"),
        ("cx grep route|path src", "grep"),
    ]
    .into_iter()
    .enumerate()
    {
        let receipt = record_command_report(&CommandReportRecord {
            command,
            command_family,
            command_shape: "",
            command_shape_hash: "",
            issue_kind: "suspicious_output",
            note: "test report",
        })
        .unwrap();
        ids[index] = receipt.id;
    }
    ids
}

fn classify_report_fixture(report_ids: [u64; 3]) {
    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: report_ids[0],
        status: CommandReportStatus::Resolved,
        denial_reason: None,
        related_report_id: None,
        note: "fixed with token=sk-abcdefghijklmnopqrstuvwxyz",
        revision: "r111",
    })
    .unwrap();
    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: report_ids[2],
        status: CommandReportStatus::NativeParity,
        denial_reason: None,
        related_report_id: None,
        note: "native grep produced the same result",
        revision: "",
    })
    .unwrap();
}

fn assert_report_grouping() {
    let root_totals =
        command_report_totals_at_level(CommandLevel::Root, 10, CommandFilter::default()).unwrap();
    assert_eq!(root_totals.len(), 2);
    assert_eq!(root_totals[0].command, "git");
    assert_eq!(root_totals[0].reports, 2);
    assert_eq!(root_totals[0].open, 1);
    assert_eq!(root_totals[0].resolved, 1);
    assert_eq!(root_totals[1].command, "grep");
    assert_eq!(root_totals[1].reports, 1);
    assert_eq!(root_totals[1].native_parity, 1);

    let git_filter = CommandFilter {
        command_root: Some("git"),
        command: None,
    };
    let git_totals = command_report_totals_at_level(CommandLevel::Command, 10, git_filter).unwrap();
    assert_eq!(git_totals.len(), 2);
    assert!(git_totals
        .iter()
        .all(|total| total.command.starts_with("git ")));

    let git_reports = recent_command_reports_filtered(10, git_filter).unwrap();
    assert_eq!(git_reports.len(), 2);
    assert!(git_reports
        .iter()
        .all(|report| report.command_root == "git"));

    let diff_filter = CommandFilter {
        command_root: None,
        command: Some("git diff"),
    };
    let diff_reports = recent_command_reports_filtered(10, diff_filter).unwrap();
    assert_eq!(diff_reports.len(), 1);
    assert_eq!(diff_reports[0].command_family, "git diff");
    assert_eq!(diff_reports[0].status, "resolved");
    assert_eq!(
        diff_reports[0].resolution_note,
        "fixed with token=[REDACTED]"
    );
    assert_eq!(diff_reports[0].resolution_revision, "r111");
}

fn assert_report_status_filters() {
    let status = command_report_status_summary(CommandFilter::default()).unwrap();
    assert_eq!(status.total, 3);
    assert_eq!(status.open, 1);
    assert_eq!(status.resolved, 1);
    assert_eq!(status.native_parity, 1);
    assert_eq!(status.closed(), 2);

    let open_reports = recent_command_reports_filtered_by_status(
        10,
        CommandFilter::default(),
        Some(CommandReportStatus::Open),
    )
    .unwrap();
    assert_eq!(open_reports.len(), 1);
    assert_eq!(open_reports[0].command_family, "git status");
    let open_totals = command_report_totals_at_level_by_status(
        CommandLevel::Root,
        10,
        CommandFilter::default(),
        Some(CommandReportStatus::Open),
    )
    .unwrap();
    assert_eq!(open_totals.len(), 1);
    assert_eq!(open_totals[0].open, 1);
}

fn assert_report_reopen_and_missing(report_id: u64) {
    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id,
        status: CommandReportStatus::Open,
        denial_reason: None,
        related_report_id: None,
        note: "reopened after a new reproduction",
        revision: "",
    })
    .unwrap();
    let reopened = command_report_status_summary(CommandFilter::default()).unwrap();
    assert_eq!(reopened.open, 2);
    assert_eq!(reopened.resolved, 0);
    assert_eq!(reopened.total, 3);

    let missing = update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: 999,
        status: CommandReportStatus::Resolved,
        denial_reason: None,
        related_report_id: None,
        note: "missing report",
        revision: "",
    })
    .unwrap_err();
    assert!(missing
        .to_string()
        .contains("command-quality report 999 was not found"));
}

fn assert_concurrent_writer_retry(db_path: &std::path::Path, report_id: u64) {
    let path = db_path.to_path_buf();
    let (locked_sender, locked_receiver) = mpsc::channel();
    let blocker = std::thread::spawn(move || {
        let connection = rusqlite::Connection::open(path).unwrap();
        connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        locked_sender.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        connection.execute_batch("COMMIT").unwrap();
    });
    locked_receiver.recv().unwrap();

    let receipt = update_command_report_disposition(&CommandReportDispositionRecord {
        report_id,
        status: CommandReportStatus::NotReproducible,
        denial_reason: None,
        related_report_id: None,
        note: "concurrent writer released its reservation",
        revision: "r111",
    })
    .unwrap();
    assert_eq!(receipt.status, CommandReportStatus::NotReproducible);
    blocker.join().unwrap();
}

#[test]
fn legacy_report_database_reads_as_open_and_migrates_on_update() {
    let _lock = ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("legacy.sqlite");
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE command_reports (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                occurred_at_ms INTEGER NOT NULL,
                command TEXT NOT NULL,
                command_family TEXT NOT NULL,
                issue_kind TEXT NOT NULL,
                note TEXT NOT NULL
            );
            INSERT INTO command_reports (
                occurred_at_ms, command, command_family, issue_kind, note
            ) VALUES (100, 'cx git diff', 'git diff', 'suspicious_output', 'legacy report');
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

    let reports = recent_command_reports_filtered(10, CommandFilter::default()).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].status, "open");
    assert_eq!(reports[0].status_updated_at_ms, 0);

    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: 1,
        status: CommandReportStatus::Resolved,
        denial_reason: None,
        related_report_id: None,
        note: "classified after migration",
        revision: "r111",
    })
    .unwrap();
    let migrated = recent_command_reports_filtered(10, CommandFilter::default()).unwrap();
    assert_eq!(migrated[0].status, "resolved");
    assert_eq!(migrated[0].resolution_revision, "r111");

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let version: String = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'insights_schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "19");
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal");
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
