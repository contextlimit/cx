use super::*;

fn record(command: &str) -> anyhow::Result<()> {
    let raw = OutputObservation::from_text(command, "raw evidence\n");
    record_invocation(&InvocationRecord {
        command,
        exit_code: 0,
        raw: Some(&raw),
        emitted: TextMetrics::from_text("evidence\n"),
    })
}

#[test]
fn current_schema_recording_skips_schema_and_historical_rollup_writes() {
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
            record("seed").unwrap();
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            connection
                .execute_batch(
                    "
                    CREATE TRIGGER reject_redundant_schema_marker_write
                    BEFORE UPDATE ON schema_meta
                    WHEN OLD.key = 'insights_schema_version'
                    BEGIN
                        SELECT RAISE(ABORT, 'current schema marker must remain read-only');
                    END;
                    CREATE TRIGGER reject_redundant_failure_rollup
                    BEFORE UPDATE ON command_totals
                    WHEN OLD.command = 'seed'
                    BEGIN
                        SELECT RAISE(ABORT, 'historical totals must not be rebuilt');
                    END;
                    ",
                )
                .unwrap();
            drop(connection);

            record("next").unwrap();
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let version: u64 = connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key = 'insights_schema_version'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
                .parse()
                .unwrap();
            let invocations: u64 = connection
                .query_row("SELECT COUNT(*) FROM command_invocations", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let journal_mode: String = connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, store::INSIGHTS_DATABASE_SCHEMA_VERSION);
            assert_eq!(invocations, 2);
            assert_eq!(journal_mode, "wal");
        },
    );
}

#[test]
fn schema_fifteen_upgrade_skips_redundant_failure_rollup_and_enables_wal() {
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
            record("seed").unwrap();
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            connection
                .execute(
                    "UPDATE schema_meta SET value = '15' WHERE key = 'insights_schema_version'",
                    [],
                )
                .unwrap();
            let journal_mode: String = connection
                .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
                .unwrap();
            assert_eq!(journal_mode, "delete");
            connection
                .execute_batch(
                    "
                    CREATE TRIGGER reject_schema_fifteen_failure_rollup
                    BEFORE UPDATE ON command_totals
                    WHEN OLD.command = 'seed'
                    BEGIN
                        SELECT RAISE(ABORT, 'schema 15 totals are already current');
                    END;
                    ",
                )
                .unwrap();
            drop(connection);

            record("next").unwrap();
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let version: String = connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key = 'insights_schema_version'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let journal_mode: String = connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, store::INSIGHTS_DATABASE_SCHEMA_VERSION.to_string());
            assert_eq!(journal_mode, "wal");
        },
    );
}

#[test]
fn schema_seventeen_upgrade_adds_exact_report_evidence_linkage() {
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
            record("seed").unwrap();
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            connection
                .execute_batch(
                    "
                    DROP INDEX idx_command_failures_invocation;
                    ALTER TABLE command_failures DROP COLUMN invocation_id;
                    ALTER TABLE command_report_evidence DROP COLUMN evidence_kind;
                    UPDATE schema_meta
                    SET value = '17'
                    WHERE key = 'insights_schema_version';
                    ",
                )
                .unwrap();
            drop(connection);

            record("next").unwrap();
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            assert!(column_exists(&connection, "command_failures", "invocation_id").unwrap());
            assert!(
                column_exists(&connection, "command_report_evidence", "evidence_kind").unwrap()
            );
            let index_count: u64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'index' AND name = 'idx_command_failures_invocation'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(index_count, 1);
        },
    );
}

#[test]
fn newer_database_schema_is_rejected_without_writing_telemetry() {
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
            record("seed").unwrap();
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            connection
                .execute(
                    "UPDATE schema_meta SET value = '20' WHERE key = 'insights_schema_version'",
                    [],
                )
                .unwrap();
            drop(connection);

            let error = record("next").unwrap_err();
            assert!(error.to_string().contains("newer than supported schema 19"));
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let invocations: u64 = connection
                .query_row("SELECT COUNT(*) FROM command_invocations", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(invocations, 1);
        },
    );
}
