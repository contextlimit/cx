use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::support::{redaction, utils};

use super::recording::RuntimeInvocationContext;
use super::{
    bounded_failure_text, bounded_limit, create_schema, insights_db_file, now_ms,
    open_readonly_database, settings, table_exists, CommandFilter,
};

const MAX_ROUTING_ARGS: usize = 64;
const MAX_ROUTING_ARG_CHARS: usize = 512;
const MAX_ROUTING_COMMAND_CHARS: usize = 8 * 1024;
const MAX_ROUTING_SHAPE_CHARS: usize = 4 * 1024;

#[derive(Debug, Clone)]
pub struct RoutingDecisionRecord<'a> {
    pub args: &'a [String],
    pub reason: &'a str,
    pub error_kind: &'a str,
    pub explicit_auto: bool,
    pub passthrough_eligible: bool,
    pub passthrough_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingDecisionInsight {
    pub id: u64,
    pub occurred_at_ms: u64,
    pub process: String,
    pub command_family: String,
    pub command: String,
    pub argv_json: String,
    pub command_shape: String,
    pub command_shape_hash: String,
    pub decision: String,
    pub reason: String,
    pub error_kind: String,
    pub explicit_auto: bool,
    pub passthrough_eligible: bool,
    pub passthrough_enabled: bool,
    pub thread_id: String,
    pub plan_title: String,
    pub plan_project_id: String,
    pub plan_folder_id: String,
    pub cwd: String,
    pub git_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingDecisionTotalInsight {
    pub command_root: String,
    pub command_family: String,
    pub reason: String,
    pub error_kind: String,
    pub decisions: u64,
    pub latest_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoutingDecisionSummary {
    pub rejections: u64,
    pub passthrough_eligible: u64,
    pub passthrough_disabled: u64,
    pub cx_owned_errors: u64,
    pub latest_at_ms: u64,
}

pub fn record_routing_rejection(record: &RoutingDecisionRecord<'_>) -> Result<bool> {
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
    insert_routing_rejection(&connection, record, settings)?;
    Ok(true)
}

pub fn recent_routing_decisions(
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<RoutingDecisionInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_routing_decisions")? {
        return Ok(Vec::new());
    }

    let mut statement = connection.prepare(
        "
        SELECT id, occurred_at_ms, process, command_family, command, argv_json,
               command_shape, command_shape_hash, decision, reason, error_kind,
               explicit_auto, passthrough_eligible, passthrough_enabled,
               thread_id, plan_title, plan_project_id, plan_folder_id, cwd, git_root
        FROM command_routing_decisions
        WHERE (?1 IS NULL OR process = ?1)
          AND (?2 IS NULL OR command_family = ?2)
        ORDER BY occurred_at_ms DESC, id DESC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.command_root,
            filter.command,
            bounded_limit(limit.clamp(1, 100))
        ],
        |row| {
            Ok(RoutingDecisionInsight {
                id: row.get(0)?,
                occurred_at_ms: row.get(1)?,
                process: row.get(2)?,
                command_family: row.get(3)?,
                command: row.get(4)?,
                argv_json: row.get(5)?,
                command_shape: row.get(6)?,
                command_shape_hash: row.get(7)?,
                decision: row.get(8)?,
                reason: row.get(9)?,
                error_kind: row.get(10)?,
                explicit_auto: row.get::<_, i64>(11)? != 0,
                passthrough_eligible: row.get::<_, i64>(12)? != 0,
                passthrough_enabled: row.get::<_, i64>(13)? != 0,
                thread_id: row.get(14)?,
                plan_title: row.get(15)?,
                plan_project_id: row.get(16)?,
                plan_folder_id: row.get(17)?,
                cwd: row.get(18)?,
                git_root: row.get(19)?,
            })
        },
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn routing_decision_totals(
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<RoutingDecisionTotalInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_routing_decisions")? {
        return Ok(Vec::new());
    }

    let mut statement = connection.prepare(
        "
        SELECT process, command_family, reason, error_kind, COUNT(*), MAX(occurred_at_ms)
        FROM command_routing_decisions
        WHERE (?1 IS NULL OR process = ?1)
          AND (?2 IS NULL OR command_family = ?2)
        GROUP BY process, command_family, reason, error_kind
        ORDER BY COUNT(*) DESC, MAX(occurred_at_ms) DESC, process ASC, command_family ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.command_root,
            filter.command,
            bounded_limit(limit.clamp(1, 100))
        ],
        |row| {
            Ok(RoutingDecisionTotalInsight {
                command_root: row.get(0)?,
                command_family: row.get(1)?,
                reason: row.get(2)?,
                error_kind: row.get(3)?,
                decisions: row.get(4)?,
                latest_at_ms: row.get(5)?,
            })
        },
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn routing_decision_summary(filter: CommandFilter<'_>) -> Result<RoutingDecisionSummary> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(RoutingDecisionSummary::default());
    };
    if !table_exists(&connection, "command_routing_decisions")? {
        return Ok(RoutingDecisionSummary::default());
    }

    connection
        .query_row(
            "
            SELECT COUNT(*),
                   COALESCE(SUM(passthrough_eligible), 0),
                   COALESCE(SUM(CASE WHEN reason = 'passthrough-disabled' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN reason = 'cx-owned-parse-error' THEN 1 ELSE 0 END), 0),
                   COALESCE(MAX(occurred_at_ms), 0)
            FROM command_routing_decisions
            WHERE (?1 IS NULL OR process = ?1)
              AND (?2 IS NULL OR command_family = ?2)
            ",
            params![filter.command_root, filter.command],
            |row| {
                Ok(RoutingDecisionSummary {
                    rejections: row.get(0)?,
                    passthrough_eligible: row.get(1)?,
                    passthrough_disabled: row.get(2)?,
                    cx_owned_errors: row.get(3)?,
                    latest_at_ms: row.get(4)?,
                })
            },
        )
        .map_err(Into::into)
}

pub(super) fn create_routing_decisions_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS command_routing_decisions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at_ms INTEGER NOT NULL,
            process TEXT NOT NULL DEFAULT '',
            command_family TEXT NOT NULL DEFAULT '',
            command TEXT NOT NULL,
            argv_json TEXT NOT NULL DEFAULT '[]',
            command_shape TEXT NOT NULL DEFAULT '',
            command_shape_hash TEXT NOT NULL DEFAULT '',
            decision TEXT NOT NULL,
            reason TEXT NOT NULL,
            error_kind TEXT NOT NULL,
            explicit_auto INTEGER NOT NULL,
            passthrough_eligible INTEGER NOT NULL,
            passthrough_enabled INTEGER NOT NULL,
            thread_id TEXT NOT NULL DEFAULT '',
            plan_title TEXT NOT NULL DEFAULT '',
            plan_project_id TEXT NOT NULL DEFAULT '',
            plan_folder_id TEXT NOT NULL DEFAULT '',
            cwd TEXT NOT NULL DEFAULT '',
            git_root TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_command_routing_reason_time
            ON command_routing_decisions(reason, occurred_at_ms);
        CREATE INDEX IF NOT EXISTS idx_command_routing_family_time
            ON command_routing_decisions(command_family, occurred_at_ms);
        CREATE INDEX IF NOT EXISTS idx_command_routing_root_time
            ON command_routing_decisions(process, occurred_at_ms);
        ",
    )?;
    Ok(())
}

fn insert_routing_rejection(
    connection: &Connection,
    record: &RoutingDecisionRecord<'_>,
    settings: settings::InsightSettings,
) -> Result<()> {
    let bounded_args = bounded_redacted_args(record.args);
    let command_args = command_args(&bounded_args);
    let process = command_args
        .first()
        .map(String::as_str)
        .unwrap_or("unknown");
    let command_family = command_family(command_args);
    let command = if settings.record_command_text {
        bounded_failure_text(&utils::shell_join(command_args), MAX_ROUTING_COMMAND_CHARS)
    } else {
        command_family.clone()
    };
    let argv_json = if settings.record_command_text {
        serde_json::to_string(&bounded_args).unwrap_or_else(|_| "[]".to_string())
    } else {
        "[]".to_string()
    };
    let command_shape = if settings.record_command_shape {
        bounded_failure_text(
            &redaction::argument_shape_join(command_args),
            MAX_ROUTING_SHAPE_CHARS,
        )
    } else {
        String::new()
    };
    let command_shape_hash = if command_shape.is_empty() {
        String::new()
    } else {
        redaction::stable_shape_hash(&command_shape)
    };
    let runtime = RuntimeInvocationContext::current();

    connection.execute(
        "
        INSERT INTO command_routing_decisions (
            occurred_at_ms, process, command_family, command, argv_json,
            command_shape, command_shape_hash, decision, reason, error_kind,
            explicit_auto, passthrough_eligible, passthrough_enabled,
            thread_id, plan_title, plan_project_id, plan_folder_id, cwd, git_root
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'rejected', ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
        ",
        params![
            now_ms(),
            process,
            command_family,
            command,
            argv_json,
            command_shape,
            command_shape_hash,
            record.reason,
            record.error_kind,
            record.explicit_auto,
            record.passthrough_eligible,
            record.passthrough_enabled,
            runtime.thread_id,
            runtime.plan_title,
            runtime.plan_project_id,
            runtime.plan_folder_id,
            runtime.cwd,
            runtime.git_root,
        ],
    )?;
    Ok(())
}

fn bounded_redacted_args(args: &[String]) -> Vec<String> {
    let redacted = redaction::redact_argv(args);
    let mut bounded = redacted
        .iter()
        .take(MAX_ROUTING_ARGS)
        .map(|arg| truncate_chars(arg, MAX_ROUTING_ARG_CHARS))
        .collect::<Vec<_>>();
    if redacted.len() > MAX_ROUTING_ARGS {
        bounded.push(format!(
            "<{} args omitted>",
            redacted.len() - MAX_ROUTING_ARGS
        ));
    }
    bounded
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let marker = "... [truncated]";
    let keep = max_chars.saturating_sub(marker.chars().count());
    let mut result = value.chars().take(keep).collect::<String>();
    result.push_str(marker);
    result
}

fn command_args(args: &[String]) -> &[String] {
    let mut start = usize::from(args.first().is_some_and(|arg| arg == "cx"));
    if args.get(start).is_some_and(|arg| arg == "--") {
        start += 1;
    }
    &args[start..]
}

fn command_family(args: &[String]) -> String {
    let root = args.first().map(String::as_str).unwrap_or("unknown");
    let second = args.get(1).map(String::as_str);
    match (root, second) {
        ("cmake", Some("--build" | "build")) => return "cmake build".to_string(),
        ("node", Some("--check" | "-c")) => return "node --check".to_string(),
        ("cargo", Some("test")) => return "cargo test".to_string(),
        ("go", Some("test")) => return "go test".to_string(),
        ("docker", Some("ps")) => return "docker ps".to_string(),
        ("docker", Some("logs")) => return "docker logs".to_string(),
        ("kubectl", Some("logs")) => return "kubectl logs".to_string(),
        _ => {}
    }
    let preserve_subcommand = matches!(
        root,
        "git" | "cargo" | "go" | "docker" | "kubectl" | "node" | "insights"
    );
    if preserve_subcommand {
        if let Some(subcommand) = args.get(1).filter(|arg| !arg.starts_with('-')) {
            return format!("{root} {subcommand}");
        }
    }
    root.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_redacted_args_limits_count_length_and_secrets() {
        let mut args = vec!["cx".to_string(), "--".to_string(), "read".to_string()];
        args.push("sk-abcdefghijklmnopqrstuvwxyz".to_string());
        args.push("x".repeat(800));
        args.extend((0..80).map(|index| format!("arg-{index}")));

        let bounded = bounded_redacted_args(&args);
        assert_eq!(bounded.len(), MAX_ROUTING_ARGS + 1);
        assert!(!bounded.join(" ").contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(bounded.iter().all(|arg| arg.chars().count() <= 512));
        assert!(bounded.last().unwrap().contains("args omitted"));
    }

    #[test]
    fn routing_family_keeps_only_real_command_tree_subcommands() {
        assert_eq!(command_family(&["read".into(), "file.rs".into()]), "read");
        assert_eq!(
            command_family(&["git".into(), "branch".into(), "--show-current".into()]),
            "git branch"
        );
        assert_eq!(
            command_family(&["node".into(), "--check".into()]),
            "node --check"
        );
        assert_eq!(
            command_family(&["cmake".into(), "--build".into()]),
            "cmake build"
        );
    }
}
