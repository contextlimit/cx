use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use super::{
    bounded_failure_text, command_root, create_schema, insights_db_file, now_ms, settings,
    MAX_FAILURE_RESPONSE_CHARS,
};

#[derive(Debug, Clone)]
pub struct CommandRepairRecord<'a> {
    pub process: &'a str,
    pub command_family: &'a str,
    pub command: &'a str,
    pub source: &'a str,
    pub rule_id: &'a str,
    pub action: &'a str,
    pub original_exit_code: i32,
    pub final_exit_code: i32,
    pub original_response: &'a str,
    pub final_response: &'a str,
}

pub fn record_command_repair(record: &CommandRepairRecord<'_>) -> Result<bool> {
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
    insert_command_repair(&connection, record, settings)?;
    Ok(true)
}

pub(super) fn create_command_repairs_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS command_repairs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at_ms INTEGER NOT NULL,
            process TEXT NOT NULL DEFAULT '',
            command_family TEXT NOT NULL DEFAULT '',
            command_root TEXT NOT NULL DEFAULT '',
            command TEXT NOT NULL,
            source TEXT NOT NULL,
            rule_id TEXT NOT NULL,
            action TEXT NOT NULL,
            original_exit_code INTEGER NOT NULL,
            final_exit_code INTEGER NOT NULL,
            original_response TEXT NOT NULL,
            final_response TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_command_repairs_family_time
            ON command_repairs(command_family, occurred_at_ms);
        CREATE INDEX IF NOT EXISTS idx_command_repairs_rule_time
            ON command_repairs(rule_id, occurred_at_ms);
        ",
    )?;
    Ok(())
}

fn insert_command_repair(
    connection: &Connection,
    record: &CommandRepairRecord<'_>,
    settings: settings::InsightSettings,
) -> Result<()> {
    let command = if settings.record_command_text {
        record.command
    } else {
        record.command_family
    };
    let source = if settings.record_sources {
        record.source
    } else {
        "not recorded"
    };
    let original_response = if settings.record_failure_responses {
        bounded_failure_text(record.original_response, MAX_FAILURE_RESPONSE_CHARS)
    } else {
        String::new()
    };
    let final_response = if settings.record_failure_responses {
        bounded_failure_text(record.final_response, MAX_FAILURE_RESPONSE_CHARS)
    } else {
        String::new()
    };
    connection.execute(
        "
        INSERT INTO command_repairs (
            occurred_at_ms, process, command_family, command_root, command, source,
            rule_id, action, original_exit_code, final_exit_code, original_response, final_response
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
        params![
            now_ms(),
            record.process,
            record.command_family,
            command_root(record.command_family),
            command,
            source,
            record.rule_id,
            record.action,
            record.original_exit_code,
            record.final_exit_code,
            original_response,
            final_response,
        ],
    )?;
    Ok(())
}
