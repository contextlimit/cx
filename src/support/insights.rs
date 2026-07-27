use crate::support::paths::global_db_file;
use crate::support::redaction;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[path = "insights_archive.rs"]
mod archive;
#[path = "insights_artifacts.rs"]
mod artifacts;
#[path = "insights_failure_detail.rs"]
mod failure_detail;
#[path = "insights_models.rs"]
mod models;
#[path = "insights_opportunities.rs"]
mod opportunities;
#[path = "insights_queries.rs"]
mod queries;
#[path = "insights_query_api.rs"]
mod query_api;
#[path = "insights_recording.rs"]
mod recording;
#[path = "insights_repairs.rs"]
mod repairs;
#[path = "insights_report_triage.rs"]
mod report_triage;
#[path = "insights_reports.rs"]
mod reports;
#[path = "insights_routing.rs"]
mod routing;
#[path = "insights_settings.rs"]
mod settings;
#[path = "insights_store.rs"]
mod store;
pub use archive::{
    archive_summary, ArchiveCommandQualityInsight, ArchiveCommandTotalInsight,
    ArchiveCoverageInsight, ArchiveDedupeInsight, ArchiveOverallInsight, ArchivePathInsight,
    ArchiveSourceTotalInsight, ArchiveSummary, ArchiveSupportClassInsight,
};
pub use artifacts::{recent_failure_artifacts, record_failure_artifact};
use failure_detail::{
    failure_artifact_ref, insert_failure_detail, prepare_failure_detail,
    should_prepare_failure_detail,
};
use models::{ratio, ExpansionMetrics, SavedMetrics};
pub use models::{
    CommandFilter, CommandLevel, CommandReportDenialReason, CommandReportDenialReasonSummary,
    CommandReportDispositionReceipt, CommandReportDispositionRecord, CommandReportInsight,
    CommandReportReceipt, CommandReportRecord, CommandReportStatus, CommandReportStatusSummary,
    CommandReportTotalInsight, CommandReportTriageProposal, CommandReportTriageReceipt,
    CommandTotalInsight, CommandTotalSort, DailyInsight, FailureArtifactInsight,
    FailureArtifactRecord, FailureCoverageInsight, FailureDetailInsight, FailureDetailRecord,
    InvocationInsight, OverallInsight, SavingsDistributionInsight, SavingsSort, TextMetrics,
    GENERIC_COMMAND_REPORT_NOTE,
};
pub use opportunities::{
    command_opportunities, command_opportunities_filtered, record_command_opportunity,
    CommandOpportunityInsight, CommandOpportunityRecord, OpportunityConfidence, OpportunityFilter,
};
use query_api::bounded_limit;
pub use query_api::{
    command_root, command_totals, command_totals_at_level, command_totals_at_level_filtered,
    daily_totals, daily_totals_filtered, expansion_invocations, expansion_invocations_filtered,
    failure_coverage_at_level_filtered, insights_database_path, largest_invocations,
    largest_invocations_filtered, overall_totals, overall_totals_filtered, recent_failure_details,
    recent_invocations, recent_invocations_filtered, savings_distribution,
    savings_distribution_filtered,
};
use recording::PreparedInvocation;
pub use recording::{InvocationContext, InvocationRecord, OutputObservation};
pub use repairs::{record_command_repair, CommandRepairRecord};
pub use reports::{
    command_report_denial_reason_summary, command_report_status_summary,
    command_report_totals_at_level, command_report_totals_at_level_by_status,
    recent_command_reports, recent_command_reports_filtered,
    recent_command_reports_filtered_by_status,
};
pub use routing::{
    recent_routing_decisions, record_routing_rejection, routing_decision_summary,
    routing_decision_totals, RoutingDecisionInsight, RoutingDecisionRecord, RoutingDecisionSummary,
    RoutingDecisionTotalInsight,
};
pub use settings::{InsightSettingRow, InsightSettingsSnapshot};
use store::{column_exists, open_readonly_database, table_exists};
use store::{create_schema, insert_prepared_record};
const MAX_FAILURE_COMMAND_CHARS: usize = 16 * 1024;
const MAX_FAILURE_RESPONSE_CHARS: usize = 64 * 1024;
const MAX_REPORT_RESOLUTION_NOTE_CHARS: usize = 16 * 1024;
const MAX_REPORT_REVISION_CHARS: usize = 256;
const COMMAND_REPORT_BUSY_TIMEOUT: Duration = Duration::from_secs(10);

pub fn record_invocation(record: &InvocationRecord<'_>) -> Result<()> {
    record_invocation_with_context(record, None)
}

pub fn record_invocation_with_context(
    record: &InvocationRecord<'_>,
    context: Option<&InvocationContext<'_>>,
) -> Result<()> {
    record_invocation_with_context_and_failure(record, context, None)
}

pub fn record_invocation_with_context_and_failure(
    record: &InvocationRecord<'_>,
    context: Option<&InvocationContext<'_>>,
    failure: Option<&FailureDetailRecord<'_>>,
) -> Result<()> {
    let settings = settings::recording_settings()?;
    let should_record_failure = failure
        .map(|detail| should_prepare_failure_detail(detail, settings))
        .unwrap_or(false);
    let artifact_ref = failure
        .map(|detail| failure_artifact_ref(detail.cx_response))
        .unwrap_or_default();
    if !settings.record_invocations && !should_record_failure {
        return Ok(());
    }
    let prepared = PreparedInvocation::from_record(record, context, settings);
    let prepared_failure = failure.and_then(|detail| {
        should_record_failure.then(|| prepare_failure_detail(detail, settings, prepared.now_ms))
    });

    if settings.record_invocations || prepared_failure.is_some() {
        let db_path = insights_db_file()?;
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut connection = Connection::open(&db_path)
            .with_context(|| format!("failed to open {}", db_path.display()))?;
        connection.busy_timeout(Duration::from_millis(100))?;
        create_schema(&connection)?;
        let invocation_id = if settings.record_invocations {
            Some(insert_prepared_record(&mut connection, &prepared)?)
        } else {
            None
        };
        if let Some(detail) = prepared_failure.as_ref() {
            insert_failure_detail(&connection, detail, invocation_id)?;
        }
        artifacts::link_failure_artifact(
            &connection,
            &artifact_ref,
            invocation_id,
            None,
            record.exit_code,
        )?;
        if let Some(invocation_id) = invocation_id {
            if reports::pending_report_evidence_may_exist(
                settings.pending_report_evidence_at_ms,
                prepared.now_ms,
            ) {
                reports::backfill_recent_command_report_evidence(
                    &mut connection,
                    invocation_id,
                    &prepared,
                )?;
            }
        }
    }

    Ok(())
}

pub fn record_command_report(record: &CommandReportRecord<'_>) -> Result<CommandReportReceipt> {
    if insights_disabled() {
        return Ok(CommandReportReceipt {
            recorded: false,
            id: 0,
            occurred_at_ms: 0,
            command_family: record.command_family.to_string(),
            total_reports: 0,
            family_reports: 0,
        });
    }

    let db_path = insights_db_file()?;
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut connection = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection.busy_timeout(COMMAND_REPORT_BUSY_TIMEOUT)?;
    create_schema(&connection)?;
    let runtime = recording::RuntimeInvocationContext::current();
    let receipt = reports::insert_command_report(&mut connection, record, &runtime)?;
    Ok(receipt)
}

pub fn update_command_report_disposition(
    record: &CommandReportDispositionRecord<'_>,
) -> Result<CommandReportDispositionReceipt> {
    let db_path = insights_db_file()?;
    if !db_path.is_file() {
        bail!(
            "CX insights database does not exist at {}",
            db_path.display()
        );
    }
    let mut connection = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection.busy_timeout(COMMAND_REPORT_BUSY_TIMEOUT)?;
    create_schema(&connection)?;
    let note = bounded_failure_text(
        &redaction::redact_text(record.note),
        MAX_REPORT_RESOLUTION_NOTE_CHARS,
    );
    let revision = bounded_failure_text(
        &redaction::redact_text(record.revision),
        MAX_REPORT_REVISION_CHARS,
    );
    reports::upsert_command_report_disposition(
        &mut connection,
        &CommandReportDispositionRecord {
            report_id: record.report_id,
            status: record.status,
            denial_reason: record.denial_reason,
            related_report_id: record.related_report_id,
            note: &note,
            revision: &revision,
        },
    )
}

pub fn triage_command_reports(apply: bool) -> Result<CommandReportTriageReceipt> {
    report_triage::command_report_triage(apply)
}

pub fn record_failure_detail(record: &FailureDetailRecord<'_>) -> Result<bool> {
    let settings = settings::recording_settings()?;
    if !should_prepare_failure_detail(record, settings) {
        return Ok(false);
    }
    let detail = prepare_failure_detail(record, settings, now_ms());

    let db_path = insights_db_file()?;
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let connection = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection.busy_timeout(Duration::from_millis(100))?;
    create_schema(&connection)?;
    insert_failure_detail(&connection, &detail, None)?;
    artifacts::link_failure_artifact(
        &connection,
        &detail.artifact_ref,
        None,
        None,
        detail.exit_code,
    )?;
    Ok(true)
}

pub fn insight_settings_snapshot() -> Result<InsightSettingsSnapshot> {
    settings::settings_snapshot()
}

pub fn set_insight_setting(key: &str, value: &str) -> Result<InsightSettingsSnapshot> {
    settings::set_setting(key, value)
}

pub fn set_insight_settings(assignments: &[(&str, &str)]) -> Result<InsightSettingsSnapshot> {
    settings::set_settings(assignments)
}

pub fn unsupported_passthrough_enabled() -> Result<bool> {
    settings::unsupported_passthrough_enabled()
}

pub fn command_optimizations_enabled() -> Result<bool> {
    settings::command_optimizations_enabled()
}

pub fn compact_document_search_results_enabled() -> Result<bool> {
    settings::compact_document_search_results_enabled()
}

pub fn exit_code_is_actionable_failure(command: &str, exit_code: i32) -> bool {
    if matches!(command_root(command), "grep" | "rg") {
        exit_code > 1
    } else {
        exit_code != 0
    }
}

fn bounded_failure_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let marker = "\n... [truncated for cx failure telemetry]";
    let marker_chars = marker.chars().count();
    if max_chars <= marker_chars {
        return text.chars().take(max_chars).collect();
    }
    let keep = max_chars - marker_chars;
    let mut bounded = text.chars().take(keep).collect::<String>();
    bounded.push_str(marker);
    bounded
}

fn insights_disabled() -> bool {
    env::var("CX_DISABLE_INSIGHTS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn insights_db_file() -> Result<PathBuf> {
    if let Some(path) = env::var_os("CX_INSIGHTS_DB_PATH") {
        return Ok(PathBuf::from(path));
    }
    let path = global_db_file()?;
    #[cfg(test)]
    let path = isolate_operator_database_in_unit_tests(path);
    Ok(path)
}

#[cfg(test)]
fn isolate_operator_database_in_unit_tests(mut path: PathBuf) -> PathBuf {
    let Some(build_home) = option_env!("HOME") else {
        return path;
    };
    let operator_database = PathBuf::from(build_home).join(".cx/db.sqlite");
    if path == operator_database {
        path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/cx-unit-test-state")
            .join(std::process::id().to_string())
            .join("db.sqlite");
    }
    path
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
#[path = "insights_tests.rs"]
mod report_failure_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_invocation_writes_row_and_rollup() {
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
                let raw_text = "line one\nline two\nline three\n";
                let emitted = TextMetrics::from_text("line one\n");
                let raw = OutputObservation::from_text("test raw", raw_text);
                record_invocation(&InvocationRecord {
                    command: "read",
                    exit_code: 0,
                    raw: Some(&raw),
                    emitted,
                })
                .unwrap();

                let connection = Connection::open(&db_path).unwrap();
                let rows: i64 = connection
                    .query_row("SELECT COUNT(*) FROM command_invocations", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                let saved_lines: i64 = connection
                    .query_row(
                        "SELECT saved_lines FROM command_totals WHERE command = 'read'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(rows, 1);
                assert_eq!(saved_lines, 2);
            },
        );
    }

    #[test]
    fn record_invocation_with_context_writes_root_and_redacted_argv() {
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
                let raw = OutputObservation::from_text("git diff", "a\nb\nc\n");
                record_invocation_with_context(
                    &InvocationRecord {
                        command: "git diff",
                        exit_code: 0,
                        raw: Some(&raw),
                        emitted: TextMetrics::from_text("a\n"),
                    },
                    Some(&InvocationContext {
                        process: "git",
                        command: "git diff --token [REDACTED]",
                        argv_json: r#"["cx","git","diff","--token","[REDACTED]"]"#,
                        emitted_response: Some("a\n"),
                    }),
                )
                .unwrap();

                let connection = Connection::open(&db_path).unwrap();
                let stored = connection
                    .query_row(
                        "SELECT process, command_family, command, argv_json FROM command_invocations",
                        [],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, String>(3)?,
                            ))
                        },
                    )
                    .unwrap();
                assert_eq!(stored.0, "git");
                assert_eq!(stored.1, "git diff");
                assert_eq!(stored.2, "git diff --token [REDACTED]");
                assert!(stored.3.contains("[REDACTED]"));

                let root_totals =
                    command_totals_at_level(CommandLevel::Root, CommandTotalSort::Lines, 10)
                        .unwrap();
                assert_eq!(root_totals[0].command, "git");
                assert_eq!(root_totals[0].saved.lines, 2);
            },
        );
    }

    #[test]
    fn old_invocation_rows_are_backfilled_with_readable_identity_columns() {
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
                let connection = Connection::open(&db_path).unwrap();
                connection
                    .execute_batch(
                        "
                        CREATE TABLE command_invocations (
                            id INTEGER PRIMARY KEY AUTOINCREMENT,
                            occurred_at_ms INTEGER NOT NULL,
                            command TEXT NOT NULL,
                            raw_source TEXT NOT NULL,
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
                            expanded_tokens INTEGER NOT NULL DEFAULT 0,
                            savings_ratio REAL NOT NULL,
                            compression_ratio REAL NOT NULL
                        );
                        INSERT INTO command_invocations (
                            occurred_at_ms, command, raw_source, exit_code,
                            raw_bytes, raw_chars, raw_lines, raw_tokens,
                            emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
                            saved_bytes, saved_chars, saved_lines, saved_tokens,
                            savings_ratio, compression_ratio
                        ) VALUES
                        (
                            1, 'git diff', 'git diff', 0,
                            10, 10, 2, 2,
                            5, 5, 1, 1,
                            5, 5, 1, 1,
                            0.5, 0.5
                        ),
                        (
                            2, 'git status', 'git status', 0,
                            0, 0, 0, 0,
                            20, 20, 1, 4,
                            0, 0, 0, 0,
                            0.0, 1.0
                        );
                        ",
                    )
                    .unwrap();
                drop(connection);

                let raw = OutputObservation::from_text("read source", "a\nb\n");
                record_invocation(&InvocationRecord {
                    command: "read",
                    exit_code: 0,
                    raw: Some(&raw),
                    emitted: TextMetrics::from_text("a\n"),
                })
                .unwrap();

                let connection = Connection::open(&db_path).unwrap();
                let old_row = connection
                    .query_row(
                        "SELECT process, command_family, argv_json, source FROM command_invocations WHERE command = 'git diff'",
                        [],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, String>(3)?,
                            ))
                        },
                    )
                    .unwrap();
                assert_eq!(old_row.0, "git");
                assert_eq!(old_row.1, "git diff");
                assert_eq!(old_row.2, "[]");
                assert_eq!(old_row.3, "git diff");

                assert_legacy_expansion_backfilled(&connection);
            },
        );
    }

    fn assert_legacy_expansion_backfilled(connection: &Connection) {
        let expansion = connection
            .query_row(
                "SELECT expanded_bytes, expanded_chars, expanded_lines,
                        expanded_tokens, expansion_reason
                 FROM command_invocations WHERE command = 'git status'",
                [],
                |row| {
                    Ok((
                        row.get::<_, u64>(0)?,
                        row.get::<_, u64>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, u64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            expansion,
            (20, 20, 1, 4, "legacy-unclassified-expansion".to_string())
        );
    }

    #[test]
    fn read_only_queries_return_command_invocation_and_daily_totals() {
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
                let read_raw = OutputObservation::from_text("read source", "a\nb\nc\nd\n");
                record_invocation(&InvocationRecord {
                    command: "read",
                    exit_code: 0,
                    raw: Some(&read_raw),
                    emitted: TextMetrics::from_text("a\n"),
                })
                .unwrap();
                let grep_raw = OutputObservation::from_text("grep", "one\ntwo\nthree\n");
                record_invocation(&InvocationRecord {
                    command: "grep",
                    exit_code: 1,
                    raw: Some(&grep_raw),
                    emitted: TextMetrics::from_text("one\n"),
                })
                .unwrap();
                let grep_error_raw = OutputObservation::from_text("grep", "regex parse error\n");
                record_invocation(&InvocationRecord {
                    command: "grep",
                    exit_code: 2,
                    raw: Some(&grep_error_raw),
                    emitted: TextMetrics::from_text("regex parse error\n"),
                })
                .unwrap();

                let totals = command_totals(CommandTotalSort::Lines, 10).unwrap();
                assert_eq!(totals[0].command, "read");
                assert_eq!(totals[0].saved.lines, 3);
                let failure_totals = command_totals(CommandTotalSort::Failures, 10).unwrap();
                let grep_total = failure_totals
                    .iter()
                    .find(|total| total.command == "grep")
                    .unwrap();
                assert_eq!(grep_total.invocations, 2);
                assert_eq!(grep_total.failures, 1);

                let largest = largest_invocations(SavingsSort::Lines, 10).unwrap();
                assert_eq!(largest[0].command, "read");
                assert_eq!(largest[0].saved.lines, 3);

                let recent = recent_invocations(10).unwrap();
                assert_eq!(recent[0].command, "grep");
                assert_eq!(recent[0].exit_code, 2);
                assert_eq!(recent[1].command, "grep");
                assert_eq!(recent[1].exit_code, 1);
                assert_eq!(recent[2].command, "read");

                let daily = daily_totals(2).unwrap();
                assert_eq!(daily.len(), 1);
                assert_eq!(daily[0].invocations, 3);
                assert_eq!(daily[0].failures, 1);

                let overall = overall_totals().unwrap();
                assert_eq!(overall.invocations, 3);
                assert_eq!(overall.failures, 1);

                let connection = Connection::open(&db_path).unwrap();
                let stored_failures: i64 = connection
                    .query_row(
                        "SELECT failures FROM command_totals WHERE command = 'grep'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(stored_failures, 1);
            },
        );
    }
}
