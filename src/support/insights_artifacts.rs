use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use super::recording::{BinaryIdentity, RuntimeInvocationContext};
use super::settings;
use super::{
    bounded_limit, create_schema, insights_db_file, now_ms, open_readonly_database, table_exists,
    FailureArtifactInsight, FailureArtifactRecord,
};

pub fn record_failure_artifact(record: &FailureArtifactRecord<'_>) -> Result<bool> {
    let settings = settings::recording_settings()?;
    if !settings.record_invocations {
        return Ok(false);
    }
    let db_path = insights_db_file()?;
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let connection = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection.busy_timeout(Duration::from_millis(100))?;
    create_schema(&connection)?;
    let runtime = RuntimeInvocationContext::current();
    let binary = BinaryIdentity::current();
    connection.execute(
        "
        INSERT INTO failure_artifacts (
            created_at_ms, display_path, tool_name, compression,
            stdout_bytes, stderr_bytes, original_bytes, stored_bytes,
            cwd, git_root, thread_id, binary_revision, binary_fingerprint
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(display_path) DO UPDATE SET
            stored_bytes = excluded.stored_bytes,
            compression = excluded.compression
        ",
        params![
            now_ms(),
            record.display_path,
            record.tool_name,
            record.compression,
            record.stdout_bytes,
            record.stderr_bytes,
            record.original_bytes,
            record.stored_bytes,
            runtime.cwd,
            runtime.git_root,
            runtime.thread_id,
            binary.revision,
            binary.fingerprint,
        ],
    )?;
    Ok(true)
}

pub(super) fn link_failure_artifact(
    connection: &Connection,
    display_path: &str,
    invocation_id: Option<u64>,
    report_id: Option<u64>,
    exit_code: i32,
) -> Result<()> {
    if display_path.is_empty() || !table_exists(connection, "failure_artifacts")? {
        return Ok(());
    }
    connection.execute(
        "
        UPDATE failure_artifacts
        SET invocation_id = COALESCE(?2, invocation_id),
            report_id = COALESCE(?3, report_id),
            exit_code = CASE WHEN ?4 = 0 THEN exit_code ELSE ?4 END
        WHERE display_path = ?1
        ",
        params![display_path, invocation_id, report_id, exit_code],
    )?;
    Ok(())
}

pub fn recent_failure_artifacts(limit: usize) -> Result<Vec<FailureArtifactInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "failure_artifacts")? {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "
        SELECT id, created_at_ms, display_path, tool_name, compression,
               stdout_bytes, stderr_bytes, original_bytes, stored_bytes,
               invocation_id, report_id, exit_code, binary_revision, binary_fingerprint
        FROM failure_artifacts
        ORDER BY created_at_ms DESC, id DESC
        LIMIT ?1
        ",
    )?;
    let rows = statement.query_map([bounded_limit(limit)], |row| {
        Ok(FailureArtifactInsight {
            id: row.get(0)?,
            created_at_ms: row.get(1)?,
            display_path: row.get(2)?,
            tool_name: row.get(3)?,
            compression: row.get(4)?,
            stdout_bytes: row.get(5)?,
            stderr_bytes: row.get(6)?,
            original_bytes: row.get(7)?,
            stored_bytes: row.get(8)?,
            invocation_id: row.get(9)?,
            report_id: row.get(10)?,
            exit_code: row.get(11)?,
            binary_revision: row.get(12)?,
            binary_fingerprint: row.get(13)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}
