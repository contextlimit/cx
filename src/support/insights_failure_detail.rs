use anyhow::Result;
use rusqlite::{params, Connection};

use crate::support::redaction;

use super::{
    bounded_failure_text, command_root, exit_code_is_actionable_failure, settings,
    FailureDetailRecord, MAX_FAILURE_COMMAND_CHARS, MAX_FAILURE_RESPONSE_CHARS,
};

#[derive(Debug, Clone)]
pub(super) struct PreparedFailureDetail {
    pub(super) occurred_at_ms: u64,
    pub(super) command_root: String,
    pub(super) command_family: String,
    pub(super) command_line: String,
    pub(super) exit_code: i32,
    pub(super) cx_response: String,
    pub(super) raw_source: String,
    pub(super) raw_response: String,
    pub(super) artifact_ref: String,
}

pub(super) fn insert_failure_detail(
    connection: &Connection,
    record: &PreparedFailureDetail,
    invocation_id: Option<u64>,
) -> Result<()> {
    connection.execute(
        "
        INSERT INTO command_failures (
            invocation_id, occurred_at_ms, command_root, command_family, command_line,
            exit_code, cx_response, raw_source, raw_response, artifact_ref
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ",
        params![
            invocation_id,
            record.occurred_at_ms,
            &record.command_root,
            &record.command_family,
            &record.command_line,
            record.exit_code,
            &record.cx_response,
            &record.raw_source,
            &record.raw_response,
            &record.artifact_ref,
        ],
    )?;
    Ok(())
}

pub(super) fn should_prepare_failure_detail(
    record: &FailureDetailRecord<'_>,
    settings: settings::InsightSettings,
) -> bool {
    settings.record_failures
        && exit_code_is_actionable_failure(record.command_family, record.exit_code)
}

pub(super) fn prepare_failure_detail(
    record: &FailureDetailRecord<'_>,
    settings: settings::InsightSettings,
    occurred_at_ms: u64,
) -> PreparedFailureDetail {
    let command_line = if settings.record_command_text {
        bounded_failure_text(
            &redaction::redact_text(record.command_line),
            MAX_FAILURE_COMMAND_CHARS,
        )
    } else {
        record.command_family.to_string()
    };
    let cx_response = if settings.record_failure_responses {
        bounded_failure_text(
            &redaction::redact_text(record.cx_response),
            MAX_FAILURE_RESPONSE_CHARS,
        )
    } else {
        String::new()
    };
    let raw_source = if settings.record_sources {
        redaction::redact_text(record.raw_source.unwrap_or("unavailable"))
    } else {
        "not recorded".to_string()
    };
    let raw_response = if settings.record_failure_responses {
        record
            .raw_response
            .map(redaction::redact_text)
            .map(|response| bounded_failure_text(&response, MAX_FAILURE_RESPONSE_CHARS))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let artifact_ref = failure_artifact_ref(record.cx_response);
    PreparedFailureDetail {
        occurred_at_ms,
        command_root: command_root(record.command_family).to_string(),
        command_family: record.command_family.to_string(),
        command_line,
        exit_code: record.exit_code,
        cx_response,
        raw_source,
        raw_response,
        artifact_ref,
    }
}

pub(super) fn failure_artifact_ref(text: &str) -> String {
    const PREFIX: &str = "[full output: ";
    let Some(start) = text.find(PREFIX) else {
        return String::new();
    };
    let after_prefix = &text[start + PREFIX.len()..];
    after_prefix
        .find(']')
        .map(|end| after_prefix[..end].trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}
