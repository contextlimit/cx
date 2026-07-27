use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use super::queries::{classified_failure_sql, command_family_query_expression, legacy_process_sql};
use super::{insights_db_file, opportunities, repairs, routing, settings, PreparedInvocation};

pub(super) const INSIGHTS_DATABASE_SCHEMA_VERSION: u64 = 20;
const FAILURE_ROLLUP_SCHEMA_VERSION: u64 = 15;
const WAL_SCHEMA_VERSION: u64 = 17;

pub(super) fn open_readonly_database() -> Result<Option<Connection>> {
    let path = insights_db_file()?;
    if !path.is_file() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(Some(connection))
}

pub(super) fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    let exists: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(exists > 0)
}

pub(super) fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for candidate in columns {
        if candidate? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

const INSIGHTS_SCHEMA_SQL: &str = "
        CREATE TABLE IF NOT EXISTS schema_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS command_invocations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at_ms INTEGER NOT NULL,
            process TEXT NOT NULL DEFAULT '',
            command_family TEXT NOT NULL DEFAULT '',
            command TEXT NOT NULL,
            argv_json TEXT NOT NULL DEFAULT '[]',
            command_shape TEXT NOT NULL DEFAULT '',
            command_shape_hash TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL,
            thread_id TEXT NOT NULL DEFAULT '',
            plan_title TEXT NOT NULL DEFAULT '',
            plan_project_id TEXT NOT NULL DEFAULT '',
            plan_folder_id TEXT NOT NULL DEFAULT '',
            cwd TEXT NOT NULL DEFAULT '',
            git_root TEXT NOT NULL DEFAULT '',
            binary_version TEXT NOT NULL DEFAULT '',
            binary_revision TEXT NOT NULL DEFAULT '',
            binary_fingerprint TEXT NOT NULL DEFAULT '',
            emitted_response_preview TEXT NOT NULL DEFAULT '',
            raw_response_preview TEXT NOT NULL DEFAULT '',
            exit_code INTEGER NOT NULL,
            raw_bytes INTEGER NOT NULL,
            raw_chars INTEGER NOT NULL,
            raw_lines INTEGER NOT NULL,
            raw_tokens INTEGER NOT NULL,
            emitted_bytes INTEGER NOT NULL,
            emitted_chars INTEGER NOT NULL,
            emitted_lines INTEGER NOT NULL,
            emitted_tokens INTEGER NOT NULL,
            saved_bytes INTEGER NOT NULL,
            saved_chars INTEGER NOT NULL,
            saved_lines INTEGER NOT NULL,
            saved_tokens INTEGER NOT NULL,
            expanded_bytes INTEGER NOT NULL DEFAULT 0,
            expanded_chars INTEGER NOT NULL DEFAULT 0,
            expanded_lines INTEGER NOT NULL DEFAULT 0,
            expanded_tokens INTEGER NOT NULL DEFAULT 0,
            expansion_reason TEXT NOT NULL DEFAULT '',
            savings_ratio REAL NOT NULL,
            compression_ratio REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS command_totals (
            command TEXT PRIMARY KEY,
            invocations INTEGER NOT NULL,
            failures INTEGER NOT NULL,
            expansions INTEGER NOT NULL DEFAULT 0,
            first_seen_ms INTEGER NOT NULL,
            last_seen_ms INTEGER NOT NULL,
            raw_bytes INTEGER NOT NULL,
            raw_chars INTEGER NOT NULL,
            raw_lines INTEGER NOT NULL,
            raw_tokens INTEGER NOT NULL,
            emitted_bytes INTEGER NOT NULL,
            emitted_chars INTEGER NOT NULL,
            emitted_lines INTEGER NOT NULL,
            emitted_tokens INTEGER NOT NULL,
            saved_bytes INTEGER NOT NULL,
            saved_chars INTEGER NOT NULL,
            saved_lines INTEGER NOT NULL,
            saved_tokens INTEGER NOT NULL,
            expanded_bytes INTEGER NOT NULL DEFAULT 0,
            expanded_chars INTEGER NOT NULL DEFAULT 0,
            expanded_lines INTEGER NOT NULL DEFAULT 0,
            expanded_tokens INTEGER NOT NULL DEFAULT 0,
            best_saved_chars INTEGER NOT NULL,
            best_saved_tokens INTEGER NOT NULL,
            best_expanded_tokens INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS command_failures (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            invocation_id INTEGER,
            occurred_at_ms INTEGER NOT NULL,
            command_root TEXT NOT NULL DEFAULT '',
            command_family TEXT NOT NULL,
            command_line TEXT NOT NULL,
            exit_code INTEGER NOT NULL,
            cx_response TEXT NOT NULL,
            raw_source TEXT NOT NULL,
            raw_response TEXT NOT NULL,
            artifact_ref TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_command_failures_family_time
            ON command_failures(command_family, occurred_at_ms);
        CREATE INDEX IF NOT EXISTS idx_command_failures_time
            ON command_failures(occurred_at_ms);
        CREATE TABLE IF NOT EXISTS failure_artifacts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            created_at_ms INTEGER NOT NULL,
            display_path TEXT NOT NULL UNIQUE,
            tool_name TEXT NOT NULL,
            compression TEXT NOT NULL,
            stdout_bytes INTEGER NOT NULL,
            stderr_bytes INTEGER NOT NULL,
            original_bytes INTEGER NOT NULL,
            stored_bytes INTEGER NOT NULL,
            invocation_id INTEGER,
            report_id INTEGER,
            exit_code INTEGER NOT NULL DEFAULT 0,
            cwd TEXT NOT NULL DEFAULT '',
            git_root TEXT NOT NULL DEFAULT '',
            thread_id TEXT NOT NULL DEFAULT '',
            binary_revision TEXT NOT NULL DEFAULT '',
            binary_fingerprint TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_failure_artifacts_created
            ON failure_artifacts(created_at_ms);
        CREATE INDEX IF NOT EXISTS idx_failure_artifacts_invocation
            ON failure_artifacts(invocation_id);
        ";

pub(super) fn create_schema(connection: &Connection) -> Result<()> {
    let previous_version = database_schema_version(connection)?;
    if previous_version == Some(INSIGHTS_DATABASE_SCHEMA_VERSION) {
        return Ok(());
    }
    if let Some(version) = previous_version {
        if version > INSIGHTS_DATABASE_SCHEMA_VERSION {
            bail!(
                "CX insights database schema {version} is newer than supported schema {}",
                INSIGHTS_DATABASE_SCHEMA_VERSION
            );
        }
    }

    if previous_version.is_none_or(|version| version < WAL_SCHEMA_VERSION) {
        enable_write_ahead_logging(connection)?;
    }
    connection.execute_batch(INSIGHTS_SCHEMA_SQL)?;
    super::reports::create_command_reports_schema(connection)?;
    opportunities::create_command_opportunities_schema(connection)?;
    repairs::create_command_repairs_schema(connection)?;
    routing::create_routing_decisions_schema(connection)?;
    settings::insert_default_settings(connection)?;
    migrate_schema(connection)?;
    if previous_version.is_none_or(|version| version < FAILURE_ROLLUP_SCHEMA_VERSION) {
        refresh_failure_rollups(connection)?;
    }
    write_database_schema_version(connection)
}

fn enable_write_ahead_logging(connection: &Connection) -> Result<()> {
    let journal_mode = connection.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    })?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        bail!("failed to enable WAL for CX insights database: {journal_mode}");
    }
    Ok(())
}

fn database_schema_version(connection: &Connection) -> Result<Option<u64>> {
    if !table_exists(connection, "schema_meta")? {
        return Ok(None);
    }
    let raw = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'insights_schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|value| {
        value
            .parse::<u64>()
            .with_context(|| format!("invalid CX insights database schema version {value:?}"))
    })
    .transpose()
}

fn write_database_schema_version(connection: &Connection) -> Result<()> {
    connection.execute(
        "INSERT INTO schema_meta (key, value)
         VALUES ('insights_schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [INSIGHTS_DATABASE_SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

fn migrate_schema(connection: &Connection) -> Result<()> {
    let backfill_invocation_expansion = table_exists(connection, "command_invocations")?
        && expansion_columns_missing(
            connection,
            "command_invocations",
            &[
                "expanded_bytes",
                "expanded_chars",
                "expanded_lines",
                "expanded_tokens",
                "expansion_reason",
            ],
        )?;
    let backfill_total_expansion = table_exists(connection, "command_totals")?
        && expansion_columns_missing(
            connection,
            "command_totals",
            &[
                "expansions",
                "expanded_bytes",
                "expanded_chars",
                "expanded_lines",
                "expanded_tokens",
                "best_expanded_tokens",
            ],
        )?;
    rebuild_legacy_command_invocations(connection)?;
    rebuild_command_report_dispositions(connection)?;
    ensure_schema_columns(connection)?;
    backfill_identity_columns(connection)?;
    if backfill_invocation_expansion {
        backfill_invocation_expansion_columns(connection)?;
    }
    if backfill_invocation_expansion || backfill_total_expansion {
        backfill_command_total_expansion_columns(connection)?;
    }
    create_schema_indexes(connection)?;
    Ok(())
}

fn rebuild_command_report_dispositions(connection: &Connection) -> Result<()> {
    const TABLE: &str = "command_report_dispositions";
    if !table_exists(connection, TABLE)? {
        return Ok(());
    }
    let table_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [TABLE],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_default();
    let has_denial_reason = column_exists(connection, TABLE, "denial_reason")?;
    let has_related_report_id = column_exists(connection, TABLE, "related_report_id")?;
    if has_denial_reason && has_related_report_id && table_sql.contains("'denied'") {
        return Ok(());
    }

    connection.execute_batch("SAVEPOINT cx_report_disposition_v19")?;
    let result = (|| -> Result<()> {
        connection.execute_batch(
            "
            DROP TABLE IF EXISTS command_report_dispositions_v19;
            CREATE TABLE command_report_dispositions_v19 (
                report_id INTEGER PRIMARY KEY,
                status TEXT NOT NULL CHECK (
                    status IN ('open', 'resolved', 'native_parity', 'not_reproducible', 'denied')
                ),
                note TEXT NOT NULL,
                revision TEXT NOT NULL DEFAULT '',
                denial_reason TEXT NOT NULL DEFAULT '',
                related_report_id INTEGER,
                updated_at_ms INTEGER NOT NULL,
                FOREIGN KEY(report_id) REFERENCES command_reports(id) ON DELETE CASCADE
            );
            ",
        )?;
        let denial_reason = if has_denial_reason {
            "COALESCE(denial_reason, '')"
        } else {
            "''"
        };
        let related_report_id = if has_related_report_id {
            "related_report_id"
        } else {
            "NULL"
        };
        connection.execute(
            &format!(
                "
                INSERT INTO command_report_dispositions_v19 (
                    report_id, status, note, revision, denial_reason,
                    related_report_id, updated_at_ms
                )
                SELECT report_id, status, note, revision, {denial_reason},
                       {related_report_id}, updated_at_ms
                FROM command_report_dispositions
                "
            ),
            [],
        )?;
        connection.execute_batch(
            "
            DROP TABLE command_report_dispositions;
            ALTER TABLE command_report_dispositions_v19
                RENAME TO command_report_dispositions;
            CREATE INDEX IF NOT EXISTS idx_command_report_dispositions_status_time
                ON command_report_dispositions(status, updated_at_ms);
            ",
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            connection.execute_batch("RELEASE SAVEPOINT cx_report_disposition_v19")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch(
                "ROLLBACK TO SAVEPOINT cx_report_disposition_v19;
                 RELEASE SAVEPOINT cx_report_disposition_v19;",
            );
            Err(error)
        }
    }
}

fn expansion_columns_missing(
    connection: &Connection,
    table: &str,
    columns: &[&str],
) -> Result<bool> {
    for column in columns {
        if !column_exists(connection, table, column)? {
            return Ok(true);
        }
    }
    Ok(false)
}

type SchemaColumn = (&'static str, &'static str, &'static str);

const REQUIRED_SCHEMA_COLUMNS: &[SchemaColumn] = &[
    (
        "command_invocations",
        "process",
        "ALTER TABLE command_invocations ADD COLUMN process TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "command_family",
        "ALTER TABLE command_invocations ADD COLUMN command_family TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "argv_json",
        "ALTER TABLE command_invocations ADD COLUMN argv_json TEXT NOT NULL DEFAULT '[]'",
    ),
    (
        "command_invocations",
        "command_shape",
        "ALTER TABLE command_invocations ADD COLUMN command_shape TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "command_shape_hash",
        "ALTER TABLE command_invocations ADD COLUMN command_shape_hash TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "source",
        "ALTER TABLE command_invocations ADD COLUMN source TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "thread_id",
        "ALTER TABLE command_invocations ADD COLUMN thread_id TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "plan_title",
        "ALTER TABLE command_invocations ADD COLUMN plan_title TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "plan_project_id",
        "ALTER TABLE command_invocations ADD COLUMN plan_project_id TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "plan_folder_id",
        "ALTER TABLE command_invocations ADD COLUMN plan_folder_id TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "cwd",
        "ALTER TABLE command_invocations ADD COLUMN cwd TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "git_root",
        "ALTER TABLE command_invocations ADD COLUMN git_root TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "binary_version",
        "ALTER TABLE command_invocations ADD COLUMN binary_version TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "binary_revision",
        "ALTER TABLE command_invocations ADD COLUMN binary_revision TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "binary_fingerprint",
        "ALTER TABLE command_invocations ADD COLUMN binary_fingerprint TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "emitted_response_preview",
        "ALTER TABLE command_invocations ADD COLUMN emitted_response_preview TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "raw_response_preview",
        "ALTER TABLE command_invocations ADD COLUMN raw_response_preview TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_invocations",
        "expanded_bytes",
        "ALTER TABLE command_invocations ADD COLUMN expanded_bytes INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_invocations",
        "expanded_chars",
        "ALTER TABLE command_invocations ADD COLUMN expanded_chars INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_invocations",
        "expanded_lines",
        "ALTER TABLE command_invocations ADD COLUMN expanded_lines INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_invocations",
        "expanded_tokens",
        "ALTER TABLE command_invocations ADD COLUMN expanded_tokens INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_invocations",
        "expansion_reason",
        "ALTER TABLE command_invocations ADD COLUMN expansion_reason TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_totals",
        "expansions",
        "ALTER TABLE command_totals ADD COLUMN expansions INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_totals",
        "expanded_bytes",
        "ALTER TABLE command_totals ADD COLUMN expanded_bytes INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_totals",
        "expanded_chars",
        "ALTER TABLE command_totals ADD COLUMN expanded_chars INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_totals",
        "expanded_lines",
        "ALTER TABLE command_totals ADD COLUMN expanded_lines INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_totals",
        "expanded_tokens",
        "ALTER TABLE command_totals ADD COLUMN expanded_tokens INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_totals",
        "best_expanded_tokens",
        "ALTER TABLE command_totals ADD COLUMN best_expanded_tokens INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "command_reports",
        "command_root",
        "ALTER TABLE command_reports ADD COLUMN command_root TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_failures",
        "invocation_id",
        "ALTER TABLE command_failures ADD COLUMN invocation_id INTEGER",
    ),
    (
        "command_failures",
        "command_root",
        "ALTER TABLE command_failures ADD COLUMN command_root TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_failures",
        "artifact_ref",
        "ALTER TABLE command_failures ADD COLUMN artifact_ref TEXT NOT NULL DEFAULT ''",
    ),
    (
        "command_report_evidence",
        "evidence_kind",
        "ALTER TABLE command_report_evidence ADD COLUMN evidence_kind TEXT NOT NULL DEFAULT 'legacy'",
    ),
];

fn ensure_schema_columns(connection: &Connection) -> Result<()> {
    for &(table, column, sql) in REQUIRED_SCHEMA_COLUMNS {
        ensure_column(connection, table, column, sql)?;
    }
    Ok(())
}

fn backfill_invocation_expansion_columns(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        UPDATE command_invocations
        SET expanded_bytes = CASE WHEN emitted_bytes > raw_bytes THEN emitted_bytes - raw_bytes ELSE 0 END,
            expanded_chars = CASE WHEN emitted_chars > raw_chars THEN emitted_chars - raw_chars ELSE 0 END,
            expanded_lines = CASE WHEN emitted_lines > raw_lines THEN emitted_lines - raw_lines ELSE 0 END,
            expanded_tokens = CASE WHEN emitted_tokens > raw_tokens THEN emitted_tokens - raw_tokens ELSE 0 END,
            expansion_reason = CASE
                WHEN emitted_bytes > raw_bytes OR emitted_chars > raw_chars
                  OR emitted_lines > raw_lines OR emitted_tokens > raw_tokens
                THEN 'legacy-unclassified-expansion'
                ELSE ''
            END;
        ",
    )?;
    Ok(())
}

fn backfill_command_total_expansion_columns(connection: &Connection) -> Result<()> {
    let command_family = command_family_query_expression(connection, Some("ci"))?;
    let predicate = "ci.expanded_bytes > 0 OR ci.expanded_chars > 0 OR ci.expanded_lines > 0 OR ci.expanded_tokens > 0";
    connection.execute_batch(&format!(
        "
        UPDATE command_totals AS ct
        SET expansions = (SELECT COUNT(*) FROM command_invocations AS ci WHERE {command_family} = ct.command AND ({predicate})),
            expanded_bytes = (SELECT COALESCE(SUM(ci.expanded_bytes), 0) FROM command_invocations AS ci WHERE {command_family} = ct.command),
            expanded_chars = (SELECT COALESCE(SUM(ci.expanded_chars), 0) FROM command_invocations AS ci WHERE {command_family} = ct.command),
            expanded_lines = (SELECT COALESCE(SUM(ci.expanded_lines), 0) FROM command_invocations AS ci WHERE {command_family} = ct.command),
            expanded_tokens = (SELECT COALESCE(SUM(ci.expanded_tokens), 0) FROM command_invocations AS ci WHERE {command_family} = ct.command),
            best_expanded_tokens = (SELECT COALESCE(MAX(ci.expanded_tokens), 0) FROM command_invocations AS ci WHERE {command_family} = ct.command);
        "
    ))?;
    Ok(())
}

fn create_schema_indexes(connection: &Connection) -> Result<()> {
    for sql in [
        "CREATE INDEX IF NOT EXISTS idx_command_invocations_command_time ON command_invocations(command_family, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_invocations_process_time ON command_invocations(process, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_invocations_thread_time ON command_invocations(thread_id, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_invocations_plan_time ON command_invocations(plan_project_id, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_invocations_git_root_time ON command_invocations(git_root, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_invocations_shape_time ON command_invocations(command_shape_hash, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_invocations_saved_tokens ON command_invocations(saved_tokens, id)",
        "CREATE INDEX IF NOT EXISTS idx_command_reports_root_time ON command_reports(command_root, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_failures_root_time ON command_failures(command_root, occurred_at_ms)",
        "CREATE INDEX IF NOT EXISTS idx_command_failures_invocation ON command_failures(invocation_id)",
    ] {
        connection.execute(sql, [])?;
    }
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    add_column_sql: &str,
) -> Result<()> {
    if !column_exists(connection, table, column)? {
        connection.execute(add_column_sql, [])?;
    }
    Ok(())
}

fn rebuild_legacy_command_invocations(connection: &Connection) -> Result<()> {
    if !table_exists(connection, "command_invocations")? {
        return Ok(());
    }
    let has_raw_source = column_exists(connection, "command_invocations", "raw_source")?;
    let has_command_root = column_exists(connection, "command_invocations", "command_root")?;
    if !has_raw_source && !has_command_root {
        return Ok(());
    }

    let process_expression = if has_command_root {
        format!(
            "COALESCE(NULLIF(command_root, ''), {})",
            legacy_process_sql("command")
        )
    } else {
        legacy_process_sql("command")
    };
    let command_family_expression =
        if column_exists(connection, "command_invocations", "command_family")? {
            "COALESCE(NULLIF(command_family, ''), command)".to_string()
        } else {
            "command".to_string()
        };
    let argv_expression = if column_exists(connection, "command_invocations", "argv_json")? {
        "argv_json"
    } else {
        "'[]'"
    };
    let source_expression = if has_raw_source {
        "raw_source"
    } else if column_exists(connection, "command_invocations", "source")? {
        "source"
    } else {
        "''"
    };

    connection.execute_batch(
        "
        CREATE TABLE command_invocations_v6 (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at_ms INTEGER NOT NULL,
            process TEXT NOT NULL DEFAULT '',
            command_family TEXT NOT NULL DEFAULT '',
            command TEXT NOT NULL,
            argv_json TEXT NOT NULL DEFAULT '[]',
            command_shape TEXT NOT NULL DEFAULT '',
            command_shape_hash TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL,
            exit_code INTEGER NOT NULL,
            raw_bytes INTEGER NOT NULL,
            raw_chars INTEGER NOT NULL,
            raw_lines INTEGER NOT NULL,
            raw_tokens INTEGER NOT NULL,
            emitted_bytes INTEGER NOT NULL,
            emitted_chars INTEGER NOT NULL,
            emitted_lines INTEGER NOT NULL,
            emitted_tokens INTEGER NOT NULL,
            saved_bytes INTEGER NOT NULL,
            saved_chars INTEGER NOT NULL,
            saved_lines INTEGER NOT NULL,
            saved_tokens INTEGER NOT NULL,
            savings_ratio REAL NOT NULL,
            compression_ratio REAL NOT NULL
        );
        ",
    )?;
    connection.execute(
        &format!(
            "
            INSERT INTO command_invocations_v6 (
                id, occurred_at_ms, process, command_family, command, argv_json,
                command_shape, command_shape_hash, source, exit_code,
                raw_bytes, raw_chars, raw_lines, raw_tokens,
                emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
                saved_bytes, saved_chars, saved_lines, saved_tokens,
                savings_ratio, compression_ratio
            )
            SELECT id, occurred_at_ms, {process_expression}, {command_family_expression}, command,
                   {argv_expression}, '', '', {source_expression}, exit_code,
                   raw_bytes, raw_chars, raw_lines, raw_tokens,
                   emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
                   saved_bytes, saved_chars, saved_lines, saved_tokens,
                   savings_ratio, compression_ratio
            FROM command_invocations
            "
        ),
        [],
    )?;
    connection.execute_batch(
        "
        DROP TABLE command_invocations;
        ALTER TABLE command_invocations_v6 RENAME TO command_invocations;
        ",
    )?;
    Ok(())
}

fn backfill_identity_columns(connection: &Connection) -> Result<()> {
    connection.execute(
        "UPDATE command_invocations SET command_family = command WHERE command_family = ''",
        [],
    )?;
    let invocation_process = legacy_process_sql("command_family");
    connection.execute(
        &format!(
            "UPDATE command_invocations SET process = {invocation_process} WHERE process = ''"
        ),
        [],
    )?;
    connection.execute(
        "UPDATE command_invocations SET source = 'emitted output' WHERE source = ''",
        [],
    )?;
    let report_root = legacy_process_sql("command_family");
    connection.execute(
        &format!("UPDATE command_reports SET command_root = {report_root} WHERE command_root = ''"),
        [],
    )?;
    connection.execute(
        &format!(
            "UPDATE command_failures SET command_root = {report_root} WHERE command_root = ''"
        ),
        [],
    )?;
    Ok(())
}

fn refresh_failure_rollups(connection: &Connection) -> Result<()> {
    let classified_failure = classified_failure_sql(connection, Some("ci"))?;
    let command_family = command_family_query_expression(connection, Some("ci"))?;
    let sql = format!(
        "
        UPDATE command_totals AS ct
        SET failures = (
            SELECT COALESCE(SUM({classified_failure}), 0)
            FROM command_invocations AS ci
            WHERE {command_family} = ct.command
        )
        "
    );
    connection.execute(&sql, [])?;
    Ok(())
}

pub(super) fn insert_prepared_record(
    connection: &mut Connection,
    prepared: &PreparedInvocation<'_>,
) -> Result<u64> {
    let transaction = connection.transaction()?;
    let invocation_id = insert_invocation_row(&transaction, prepared)?;
    upsert_command_total(&transaction, prepared)?;
    transaction.commit()?;
    Ok(invocation_id)
}

fn insert_invocation_row(
    transaction: &rusqlite::Transaction<'_>,
    prepared: &PreparedInvocation<'_>,
) -> Result<u64> {
    transaction.execute(
        "
        INSERT INTO command_invocations (
            occurred_at_ms, process, command_family, command, argv_json,
            command_shape, command_shape_hash, source, exit_code,
            thread_id, plan_title, plan_project_id, plan_folder_id, cwd, git_root,
            binary_version, binary_revision, binary_fingerprint,
            emitted_response_preview, raw_response_preview,
            raw_bytes, raw_chars, raw_lines, raw_tokens,
            emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
            saved_bytes, saved_chars, saved_lines, saved_tokens,
            expanded_bytes, expanded_chars, expanded_lines, expanded_tokens,
            expansion_reason, savings_ratio, compression_ratio
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39)
        ",
        params![
            prepared.now_ms,
            prepared.process,
            prepared.command_family,
            prepared.command,
            prepared.argv_json,
            prepared.command_shape,
            prepared.command_shape_hash,
            prepared.source,
            prepared.exit_code,
            prepared.thread_id,
            prepared.plan_title,
            prepared.plan_project_id,
            prepared.plan_folder_id,
            prepared.cwd,
            prepared.git_root,
            prepared.binary_version,
            prepared.binary_revision,
            prepared.binary_fingerprint,
            prepared.emitted_response_preview,
            prepared.raw_response_preview,
            prepared.raw.bytes,
            prepared.raw.chars,
            prepared.raw.lines,
            prepared.raw.tokens,
            prepared.emitted.bytes,
            prepared.emitted.chars,
            prepared.emitted.lines,
            prepared.emitted.tokens,
            prepared.saved.bytes,
            prepared.saved.chars,
            prepared.saved.lines,
            prepared.saved.tokens,
            prepared.expanded.bytes,
            prepared.expanded.chars,
            prepared.expanded.lines,
            prepared.expanded.tokens,
            prepared.expansion_reason,
            prepared.savings_ratio,
            prepared.compression_ratio,
        ],
    )?;
    Ok(transaction.last_insert_rowid().max(0) as u64)
}

fn upsert_command_total(
    transaction: &rusqlite::Transaction<'_>,
    prepared: &PreparedInvocation<'_>,
) -> Result<()> {
    transaction.execute(
        "
        INSERT INTO command_totals (
            command, invocations, failures, expansions, first_seen_ms, last_seen_ms,
            raw_bytes, raw_chars, raw_lines, raw_tokens,
            emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
            saved_bytes, saved_chars, saved_lines, saved_tokens,
            expanded_bytes, expanded_chars, expanded_lines, expanded_tokens,
            best_saved_chars, best_saved_tokens, best_expanded_tokens
        ) VALUES (?1, 1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
        ON CONFLICT(command) DO UPDATE SET
            invocations = invocations + 1,
            failures = failures + excluded.failures,
            expansions = expansions + excluded.expansions,
            last_seen_ms = excluded.last_seen_ms,
            raw_bytes = raw_bytes + excluded.raw_bytes,
            raw_chars = raw_chars + excluded.raw_chars,
            raw_lines = raw_lines + excluded.raw_lines,
            raw_tokens = raw_tokens + excluded.raw_tokens,
            emitted_bytes = emitted_bytes + excluded.emitted_bytes,
            emitted_chars = emitted_chars + excluded.emitted_chars,
            emitted_lines = emitted_lines + excluded.emitted_lines,
            emitted_tokens = emitted_tokens + excluded.emitted_tokens,
            saved_bytes = saved_bytes + excluded.saved_bytes,
            saved_chars = saved_chars + excluded.saved_chars,
            saved_lines = saved_lines + excluded.saved_lines,
            saved_tokens = saved_tokens + excluded.saved_tokens,
            expanded_bytes = expanded_bytes + excluded.expanded_bytes,
            expanded_chars = expanded_chars + excluded.expanded_chars,
            expanded_lines = expanded_lines + excluded.expanded_lines,
            expanded_tokens = expanded_tokens + excluded.expanded_tokens,
            best_saved_chars = MAX(best_saved_chars, excluded.best_saved_chars),
            best_saved_tokens = MAX(best_saved_tokens, excluded.best_saved_tokens),
            best_expanded_tokens = MAX(best_expanded_tokens, excluded.best_expanded_tokens)
        ",
        params![
            prepared.command_family,
            prepared.failure_count,
            i64::from(prepared.expanded.any()),
            prepared.now_ms,
            prepared.raw.bytes,
            prepared.raw.chars,
            prepared.raw.lines,
            prepared.raw.tokens,
            prepared.emitted.bytes,
            prepared.emitted.chars,
            prepared.emitted.lines,
            prepared.emitted.tokens,
            prepared.saved.bytes,
            prepared.saved.chars,
            prepared.saved.lines,
            prepared.saved.tokens,
            prepared.expanded.bytes,
            prepared.expanded.chars,
            prepared.expanded.lines,
            prepared.expanded.tokens,
            prepared.saved.chars,
            prepared.saved.tokens,
            prepared.expanded.tokens,
        ],
    )?;
    Ok(())
}
