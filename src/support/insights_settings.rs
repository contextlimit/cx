use std::env;
use std::fs;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags};

const RECORD_INVOCATIONS: &str = "record_invocations";
const RECORD_COMMAND_TEXT: &str = "record_command_text";
const RECORD_COMMAND_SHAPE: &str = "record_command_shape";
const RECORD_SOURCES: &str = "record_sources";
const RECORD_FAILURES: &str = "record_failures";
const RECORD_FAILURE_RESPONSES: &str = "record_failure_responses";
const RECORD_RESPONSE_PREVIEWS: &str = "record_response_previews";
const PASSTHROUGH_UNSUPPORTED_COMMANDS: &str = "passthrough_unsupported_commands";
const COMMAND_OPTIMIZATIONS: &str = "command_optimizations";
const COMPACT_DOCUMENT_SEARCH_RESULTS: &str = "compact_document_search_results";
pub(super) const PENDING_REPORT_EVIDENCE_AT_MS: &str = "pending_report_evidence_at_ms";
const EXPLICIT_SETTINGS_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const SETTING_DEFINITIONS: &[SettingDefinition] = &[
    SettingDefinition {
        key: RECORD_INVOCATIONS,
        default: true,
        description: "passively record command invocation metrics",
    },
    SettingDefinition {
        key: RECORD_COMMAND_TEXT,
        default: false,
        description: "store readable command text and argv_json",
    },
    SettingDefinition {
        key: RECORD_COMMAND_SHAPE,
        default: true,
        description: "store redacted command shape and stable shape hash",
    },
    SettingDefinition {
        key: RECORD_SOURCES,
        default: false,
        description: "store command output source or target labels",
    },
    SettingDefinition {
        key: RECORD_FAILURES,
        default: false,
        description: "passively record actionable failed command details",
    },
    SettingDefinition {
        key: RECORD_FAILURE_RESPONSES,
        default: false,
        description: "store bounded CX and non-CX failure responses",
    },
    SettingDefinition {
        key: RECORD_RESPONSE_PREVIEWS,
        default: false,
        description: "store redacted bounded emitted and native response previews",
    },
    SettingDefinition {
        key: PASSTHROUGH_UNSUPPORTED_COMMANDS,
        default: true,
        description: "direct-exec unsupported command families through CX",
    },
    SettingDefinition {
        key: COMMAND_OPTIMIZATIONS,
        default: true,
        description: "use optional CX command optimizations",
    },
    SettingDefinition {
        key: COMPACT_DOCUMENT_SEARCH_RESULTS,
        default: false,
        description: "compact grep/search results from document and text files",
    },
];

#[derive(Debug, Clone, Copy)]
struct SettingDefinition {
    key: &'static str,
    default: bool,
    description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct InsightSettings {
    pub record_invocations: bool,
    pub record_command_text: bool,
    pub record_command_shape: bool,
    pub record_sources: bool,
    pub record_failures: bool,
    pub record_failure_responses: bool,
    pub record_response_previews: bool,
    pub passthrough_unsupported_commands: bool,
    pub command_optimizations: bool,
    pub compact_document_search_results: bool,
    pub pending_report_evidence_at_ms: u64,
}

impl InsightSettings {
    fn all_enabled() -> Self {
        Self {
            record_invocations: true,
            record_command_text: true,
            record_command_shape: true,
            record_sources: true,
            record_failures: true,
            record_failure_responses: true,
            record_response_previews: true,
            passthrough_unsupported_commands: true,
            command_optimizations: true,
            compact_document_search_results: false,
            pending_report_evidence_at_ms: 0,
        }
    }

    fn setting_value(self, key: &str) -> Option<bool> {
        match key {
            RECORD_INVOCATIONS => Some(self.record_invocations),
            RECORD_COMMAND_TEXT => Some(self.record_command_text),
            RECORD_COMMAND_SHAPE => Some(self.record_command_shape),
            RECORD_SOURCES => Some(self.record_sources),
            RECORD_FAILURES => Some(self.record_failures),
            RECORD_FAILURE_RESPONSES => Some(self.record_failure_responses),
            RECORD_RESPONSE_PREVIEWS => Some(self.record_response_previews),
            PASSTHROUGH_UNSUPPORTED_COMMANDS => Some(self.passthrough_unsupported_commands),
            COMMAND_OPTIMIZATIONS => Some(self.command_optimizations),
            COMPACT_DOCUMENT_SEARCH_RESULTS => Some(self.compact_document_search_results),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightSettingRow {
    pub key: String,
    pub value: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightSettingsSnapshot {
    pub database: String,
    pub database_exists: bool,
    pub rows: Vec<InsightSettingRow>,
}

pub(super) fn recording_settings() -> Result<InsightSettings> {
    if super::insights_disabled() {
        return Ok(InsightSettings::default());
    }
    if env_truthy("CX_ENABLE_INSIGHTS") {
        return Ok(InsightSettings::all_enabled());
    }
    let db_path = super::insights_db_file()?;
    if env::var_os("CX_INSIGHTS_DB_PATH").is_some() {
        let mut settings = InsightSettings::all_enabled();
        if db_path.is_file() {
            let connection = Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            if super::table_exists(&connection, "settings")? {
                apply_explicit_settings(&connection, &mut settings)?;
            }
        }
        return Ok(settings);
    }
    if !db_path.is_file() {
        return Ok(default_settings_from_definitions());
    }
    let connection = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    if !super::table_exists(&connection, "settings")? {
        return Ok(default_settings_from_definitions());
    }
    settings_from_connection(&connection)
}

pub fn unsupported_passthrough_enabled() -> Result<bool> {
    if env_truthy("CX_ENABLE_UNSUPPORTED_PASSTHROUGH") {
        return Ok(true);
    }
    let default = default_settings_from_definitions().passthrough_unsupported_commands;
    if super::insights_disabled() {
        return Ok(default);
    }
    let db_path = super::insights_db_file()?;
    if !db_path.is_file() {
        return Ok(default);
    }
    let connection = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    if !super::table_exists(&connection, "settings")? {
        return Ok(default);
    }
    Ok(settings_from_connection(&connection)?.passthrough_unsupported_commands)
}

pub fn command_optimizations_enabled() -> Result<bool> {
    if super::insights_disabled() {
        return Ok(true);
    }
    let db_path = super::insights_db_file()?;
    if !db_path.is_file() {
        return Ok(default_settings_from_definitions().command_optimizations);
    }
    let connection = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    if !super::table_exists(&connection, "settings")? {
        return Ok(default_settings_from_definitions().command_optimizations);
    }
    Ok(settings_from_connection(&connection)?.command_optimizations)
}

pub fn compact_document_search_results_enabled() -> Result<bool> {
    if super::insights_disabled() {
        return Ok(default_settings_from_definitions().compact_document_search_results);
    }
    let db_path = super::insights_db_file()?;
    if !db_path.is_file() {
        return Ok(default_settings_from_definitions().compact_document_search_results);
    }
    let connection = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    if !super::table_exists(&connection, "settings")? {
        return Ok(default_settings_from_definitions().compact_document_search_results);
    }
    Ok(settings_from_connection(&connection)?.compact_document_search_results)
}

pub fn settings_snapshot() -> Result<InsightSettingsSnapshot> {
    let db_path = super::insights_db_file()?;
    let database_exists = db_path.is_file();
    let settings = if database_exists {
        let connection = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        if super::table_exists(&connection, "settings")? {
            if env::var_os("CX_INSIGHTS_DB_PATH").is_some() {
                let mut settings = InsightSettings::all_enabled();
                apply_explicit_settings(&connection, &mut settings)?;
                settings
            } else {
                settings_from_connection(&connection)?
            }
        } else {
            default_settings_from_definitions()
        }
    } else if env::var_os("CX_INSIGHTS_DB_PATH").is_some() {
        InsightSettings::all_enabled()
    } else {
        default_settings_from_definitions()
    };
    Ok(snapshot_from_settings(
        db_path.display().to_string(),
        database_exists,
        settings,
    ))
}

pub fn set_setting(key: &str, value: &str) -> Result<InsightSettingsSnapshot> {
    set_settings(&[(key, value)])
}

pub fn set_settings(assignments: &[(&str, &str)]) -> Result<InsightSettingsSnapshot> {
    let parsed = assignments
        .iter()
        .map(|(key, value)| Ok((setting_definition(key)?.key, parse_bool(value)?)))
        .collect::<Result<Vec<_>>>()?;
    if parsed.is_empty() {
        return settings_snapshot();
    }
    let db_path = super::insights_db_file()?;
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut connection = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection.busy_timeout(EXPLICIT_SETTINGS_BUSY_TIMEOUT)?;
    super::create_schema(&connection)?;
    let transaction = connection.transaction()?;
    let updated_at_ms = super::now_ms();
    for (key, value) in parsed {
        transaction.execute(
            "
            INSERT INTO settings (key, value, updated_at_ms)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at_ms = excluded.updated_at_ms
            ",
            params![key, bool_text(value), updated_at_ms],
        )?;
    }
    transaction.commit()?;
    drop(connection);
    settings_snapshot()
}

pub(super) fn insert_default_settings(connection: &Connection) -> Result<()> {
    for definition in SETTING_DEFINITIONS {
        connection.execute(
            "
            INSERT INTO settings (key, value, updated_at_ms)
            VALUES (?1, ?2, 0)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value
            WHERE settings.updated_at_ms = 0
            ",
            params![definition.key, bool_text(definition.default)],
        )?;
    }
    Ok(())
}

fn settings_from_connection(connection: &Connection) -> Result<InsightSettings> {
    let mut settings = default_settings_from_definitions();
    apply_explicit_settings(connection, &mut settings)?;
    Ok(settings)
}

fn apply_explicit_settings(connection: &Connection, settings: &mut InsightSettings) -> Result<()> {
    let mut statement =
        connection.prepare("SELECT key, value FROM settings WHERE updated_at_ms > 0")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (key, value) = row?;
        apply_setting_value(settings, &key, &value)?;
    }
    Ok(())
}

fn apply_setting_value(settings: &mut InsightSettings, key: &str, value: &str) -> Result<()> {
    if key == PENDING_REPORT_EVIDENCE_AT_MS {
        settings.pending_report_evidence_at_ms = value.trim().parse::<u64>().unwrap_or_default();
        return Ok(());
    }
    let parsed = parse_bool(value)?;
    match key {
        RECORD_INVOCATIONS => settings.record_invocations = parsed,
        RECORD_COMMAND_TEXT => settings.record_command_text = parsed,
        RECORD_COMMAND_SHAPE => settings.record_command_shape = parsed,
        RECORD_SOURCES => settings.record_sources = parsed,
        RECORD_FAILURES => settings.record_failures = parsed,
        RECORD_FAILURE_RESPONSES => settings.record_failure_responses = parsed,
        RECORD_RESPONSE_PREVIEWS => settings.record_response_previews = parsed,
        PASSTHROUGH_UNSUPPORTED_COMMANDS => settings.passthrough_unsupported_commands = parsed,
        COMMAND_OPTIMIZATIONS => settings.command_optimizations = parsed,
        COMPACT_DOCUMENT_SEARCH_RESULTS => settings.compact_document_search_results = parsed,
        _ => {}
    }
    Ok(())
}

fn default_settings_from_definitions() -> InsightSettings {
    let mut settings = InsightSettings::default();
    for definition in SETTING_DEFINITIONS {
        match definition.key {
            RECORD_INVOCATIONS => settings.record_invocations = definition.default,
            RECORD_COMMAND_TEXT => settings.record_command_text = definition.default,
            RECORD_COMMAND_SHAPE => settings.record_command_shape = definition.default,
            RECORD_SOURCES => settings.record_sources = definition.default,
            RECORD_FAILURES => settings.record_failures = definition.default,
            RECORD_FAILURE_RESPONSES => settings.record_failure_responses = definition.default,
            RECORD_RESPONSE_PREVIEWS => settings.record_response_previews = definition.default,
            PASSTHROUGH_UNSUPPORTED_COMMANDS => {
                settings.passthrough_unsupported_commands = definition.default;
            }
            COMMAND_OPTIMIZATIONS => settings.command_optimizations = definition.default,
            COMPACT_DOCUMENT_SEARCH_RESULTS => {
                settings.compact_document_search_results = definition.default;
            }
            _ => {}
        }
    }
    settings
}

fn snapshot_from_settings(
    database: String,
    database_exists: bool,
    settings: InsightSettings,
) -> InsightSettingsSnapshot {
    let rows = SETTING_DEFINITIONS
        .iter()
        .map(|definition| InsightSettingRow {
            key: definition.key.to_string(),
            value: settings
                .setting_value(definition.key)
                .unwrap_or(definition.default),
            description: definition.description,
        })
        .collect();
    InsightSettingsSnapshot {
        database,
        database_exists,
        rows,
    }
}

fn setting_definition(key: &str) -> Result<&'static SettingDefinition> {
    SETTING_DEFINITIONS
        .iter()
        .find(|definition| definition.key == key)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown insights setting `{}`; supported settings: {}",
                key,
                SETTING_DEFINITIONS
                    .iter()
                    .map(|definition| definition.key)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("invalid boolean setting value `{value}`; use true or false"),
    }
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn env_truthy(key: &str) -> bool {
    env::var(key)
        .map(|value| {
            matches!(
                value.as_str(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false)
}
