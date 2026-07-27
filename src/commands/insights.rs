use anyhow::Result;

use crate::support::insights::{self, CommandLevel, CommandTotalSort, SavingsSort};
use crate::support::runner::ProxyOutcome;

#[path = "insights_archive.rs"]
mod archive_view;
pub(crate) mod artifacts;
#[path = "insights_csv.rs"]
mod csv_view;
#[path = "insights_dashboard.rs"]
mod dashboard;
mod data;
mod distribution_view;
mod export;
mod export_json;
pub(crate) mod failure_coverage;
mod format_utils;
#[path = "insights_opportunities.rs"]
mod opportunity_view;
mod presentation;
mod report_triage;
pub(crate) mod report_view;
#[path = "insights_routing.rs"]
mod routing_view;
#[path = "insights_settings.rs"]
mod settings_cmd;
mod text;

use data::{build_recommendations, ExportEvidence, ExportSnapshot, RecommendationAnalysis};
pub(crate) use export::EXPORT_SCHEMA_VERSION;
use export::{format_export_csv, format_export_json};
use failure_coverage::load_failure_coverage;
use format_utils::format_count;
use presentation::{
    format_impact_bullets, format_impact_command_concentration, format_impact_headlines,
    format_presentation_demo_commands, format_presentation_headlines, format_presentation_metrics,
    format_presentation_operational_health, format_presentation_recommendations,
    format_presentation_slide_outline, format_report, PresentationMetrics,
    DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
};
use text::{
    command_level_label, command_total_sort_label, format_command_report_totals,
    format_command_reports, format_command_totals, format_daily, format_failure_focus,
    format_filter_line, format_invocations, format_overall, no_data_message,
    no_matching_data_message, savings_sort_label,
};

pub use archive_view::run_archive_summary;
pub use dashboard::run as run_dashboard;
pub use opportunity_view::{run_opportunities, run_opportunities_filtered};
pub use report_triage::{run as run_report_triage, ReportTriageFormat};
pub use routing_view::run as run_routing;
pub use settings_cmd::run_settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Csv,
}

pub fn run_summary(limit: usize) -> Result<ProxyOutcome> {
    let overall = insights::overall_totals()?;
    if overall.invocations == 0 {
        return Ok(ProxyOutcome::success(no_data_message()?));
    }
    let root_totals =
        insights::command_totals_at_level(CommandLevel::Root, CommandTotalSort::Tokens, limit)?;
    let totals =
        insights::command_totals_at_level(CommandLevel::Command, CommandTotalSort::Tokens, limit)?;
    let mut output = String::from("cx insights: summary\n");
    output.push_str(&format_overall(&overall));
    output.push('\n');
    output.push_str("\nTop command roots by saved tokens:\n");
    output.push_str(&format_command_totals(&root_totals));
    output.push('\n');
    output.push_str("\nTop commands by saved tokens:\n");
    output.push_str(&format_command_totals(&totals));
    Ok(ProxyOutcome::success(output))
}

pub fn run_top(sort: CommandTotalSort, level: CommandLevel, limit: usize) -> Result<ProxyOutcome> {
    let totals = insights::command_totals_at_level(level, sort, limit)?;
    if totals.is_empty() {
        return Ok(ProxyOutcome::success(no_data_message()?));
    }
    let mut output = format!(
        "cx insights: top {} by {}\n",
        command_level_label(level),
        command_total_sort_label(sort)
    );
    output.push_str(&format_command_totals(&totals));
    Ok(ProxyOutcome::success(output))
}

pub fn run_largest(
    sort: SavingsSort,
    limit: usize,
    filter: insights::CommandFilter<'_>,
) -> Result<ProxyOutcome> {
    let invocations = insights::largest_invocations_filtered(sort, limit, filter)?;
    if invocations.is_empty() {
        return Ok(ProxyOutcome::success(no_matching_data_message(filter)?));
    }
    let mut output = format!(
        "cx insights: largest invocations by {}\n",
        savings_sort_label(sort)
    );
    output.push_str(&format_filter_line(filter));
    output.push_str(&format_invocations(&invocations));
    Ok(ProxyOutcome::success(output))
}

pub fn run_recent(limit: usize, filter: insights::CommandFilter<'_>) -> Result<ProxyOutcome> {
    let invocations = insights::recent_invocations_filtered(limit.clamp(1, 100), filter)?;
    if invocations.is_empty() {
        return Ok(ProxyOutcome::success(no_matching_data_message(filter)?));
    }
    let mut output = String::from("cx insights: recent invocations\n");
    output.push_str(&format_filter_line(filter));
    output.push_str(&format_invocations(&invocations));
    Ok(ProxyOutcome::success(output))
}

pub fn run_daily(limit: usize) -> Result<ProxyOutcome> {
    let daily = insights::daily_totals(limit)?;
    if daily.is_empty() {
        return Ok(ProxyOutcome::success(no_data_message()?));
    }
    let mut output = String::from("cx insights: daily savings\n");
    output.push_str(&format_daily(&daily));
    Ok(ProxyOutcome::success(output))
}

pub fn run_expansions(limit: usize, filter: insights::CommandFilter<'_>) -> Result<ProxyOutcome> {
    let invocations = insights::expansion_invocations_filtered(limit.clamp(1, 100), filter)?;
    let mut output = String::from("cx insights: expanded invocations\n");
    output.push_str(&format!(
        "Database: {}\n",
        insights::insights_database_path()?.display()
    ));
    output.push_str(&format_filter_line(filter));
    if invocations.is_empty() {
        output.push_str("No expanded invocations recorded.");
        return Ok(ProxyOutcome::success(output));
    }
    output.push_str(&format_invocations(&invocations));
    Ok(ProxyOutcome::success(output))
}

pub fn run_presentation(limit: usize) -> Result<ProxyOutcome> {
    let evidence = ExportEvidence::load(limit, insights::CommandFilter::default())?;
    let snapshot = &evidence.snapshot;
    if snapshot.no_data() {
        return Ok(ProxyOutcome::success(no_data_message()?));
    }
    let metrics =
        PresentationMetrics::from_snapshot(snapshot, DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS);
    let recommendations = &evidence.recommendations[..evidence.recommendations.len().min(6)];

    let mut output = String::from("cx insights: presentation summary\n");
    output.push_str(&format_presentation_headlines(snapshot));
    output.push_str("\n\nMetric scorecard:\n");
    output.push_str(&format_presentation_metrics(&metrics));
    output.push_str("\n\nSlide outline:\n");
    output.push_str(&format_presentation_slide_outline(
        snapshot,
        recommendations,
    ));
    output.push_str("\n\nSpeaker bullets:\n");
    output.push_str(&format_impact_bullets(
        snapshot,
        DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
    ));
    output.push_str("\n\nRecommended focus areas:\n");
    output.push_str(&format_presentation_recommendations(recommendations));
    output.push_str("\n\nOperational health:\n");
    output.push_str(&format_presentation_operational_health(&evidence));
    output.push_str("\n\nDemo commands:\n");
    output.push_str(&format_presentation_demo_commands(limit));
    output.push_str("\n\nEvidence snapshot:\n");
    output.push_str(&format_overall(&snapshot.overall));
    output.push_str("\n\nTop command roots:\n");
    output.push_str(&format_command_totals(&snapshot.top_roots));
    output.push_str("\n\nTop command families:\n");
    output.push_str(&format_command_totals(&snapshot.top_commands));
    output.push_str("\n\nLargest single saves:\n");
    output.push_str(&format_invocations(&snapshot.largest_invocations));
    output.push_str("\n\nDaily totals:\n");
    output.push_str(&format_daily(&snapshot.daily_totals));
    Ok(ProxyOutcome::success(output))
}

pub fn run_report(limit: usize, filter: insights::CommandFilter<'_>) -> Result<ProxyOutcome> {
    let evidence = ExportEvidence::load(limit, filter)?;
    if evidence.snapshot.no_data() {
        return Ok(ProxyOutcome::success(no_matching_data_message(filter)?));
    }

    let mut output = String::from("cx insights: report\n");
    output.push_str(&format_filter_line(filter));
    output.push_str(&format_report(&evidence));
    Ok(ProxyOutcome::success(output))
}

pub fn run_reports(
    limit: usize,
    level: CommandLevel,
    filter: insights::CommandFilter<'_>,
    status: Option<insights::CommandReportStatus>,
) -> Result<ProxyOutcome> {
    let limit = limit.clamp(1, 100);
    let totals = insights::command_report_totals_at_level_by_status(level, limit, filter, status)?;
    let reports = insights::recent_command_reports_filtered_by_status(limit, filter, status)?;
    let status_summary = insights::command_report_status_summary(filter)?;
    let mut output = String::from("cx insights: command-quality reports\n");
    output.push_str(&format!(
        "Database: {}\nLevel: {}\nStatus filter: {}\n",
        insights::insights_database_path()?.display(),
        command_level_label(level),
        status.map_or("all", insights::CommandReportStatus::as_str),
    ));
    output.push_str(&format_filter_line(filter));
    output.push_str(&format!(
        "Lifecycle: {} total | {} open | {} resolved | {} native parity | {} not reproducible | {} denied | {} other\n",
        format_count(status_summary.total),
        format_count(status_summary.open),
        format_count(status_summary.resolved),
        format_count(status_summary.native_parity),
        format_count(status_summary.not_reproducible),
        format_count(status_summary.denied),
        format_count(status_summary.other),
    ));
    if totals.is_empty() && reports.is_empty() {
        output.push_str("No command-quality reports match the selected filters.");
        return Ok(ProxyOutcome::success(output));
    }
    output.push_str("\nReport totals:\n");
    output.push_str(&format_command_report_totals(&totals));
    output.push_str("\n\nRecent reports:\n");
    output.push_str(&format_command_reports(&reports));
    Ok(ProxyOutcome::success(output))
}

pub fn run_report_update(
    report_id: u64,
    status: insights::CommandReportStatus,
    denial_reason: Option<insights::CommandReportDenialReason>,
    related_report_id: Option<u64>,
    note: &str,
    revision: &str,
) -> Result<ProxyOutcome> {
    let receipt =
        insights::update_command_report_disposition(&insights::CommandReportDispositionRecord {
            report_id,
            status,
            denial_reason,
            related_report_id,
            note,
            revision,
        })?;
    let mut output = String::from("cx insights: command-quality report updated\n");
    output.push_str(&format!(
        "Database: {}\nReport id: {}\nFamily: {}\nStatus: {}\nUpdated at: {}\n",
        insights::insights_database_path()?.display(),
        receipt.report_id,
        receipt.command_family,
        receipt.status,
        receipt.updated_at_ms,
    ));
    if !receipt.revision.is_empty() {
        output.push_str(&format!("Revision: {}\n", receipt.revision));
    }
    if let Some(reason) = receipt.denial_reason {
        output.push_str(&format!("Denial reason: {reason}\n"));
    }
    if let Some(related_report_id) = receipt.related_report_id {
        output.push_str(&format!("Related report id: {related_report_id}\n"));
    }
    output.push_str(&format!("Note: {}", receipt.note));
    Ok(ProxyOutcome::success(output))
}

pub fn run_impact(limit: usize, context_window_tokens: u64) -> Result<ProxyOutcome> {
    let snapshot = ExportSnapshot::load(limit, insights::CommandFilter::default())?;
    if snapshot.no_data() {
        return Ok(ProxyOutcome::success(no_data_message()?));
    }

    let mut output = String::from("cx insights: impact scorecard\n");
    output.push_str(&format_impact_headlines(
        &snapshot,
        context_window_tokens.max(1),
    ));
    output.push_str("\n\nCommand concentration:\n");
    output.push_str(&format_impact_command_concentration(&snapshot));
    output.push_str("\n\nPresentation bullets:\n");
    output.push_str(&format_impact_bullets(
        &snapshot,
        context_window_tokens.max(1),
    ));
    Ok(ProxyOutcome::success(output))
}

pub fn run_recommend(limit: usize) -> Result<ProxyOutcome> {
    let analysis = RecommendationAnalysis::load(insights::CommandFilter::default())?;
    if analysis.overall.invocations == 0 {
        return Ok(ProxyOutcome::success(no_data_message()?));
    }

    let recommendations = build_recommendations(&analysis, limit.clamp(1, 12));
    let mut output = String::from("cx insights: recommendations\n");
    output.push_str(&format!(
        "Database: {}\nBasis: {} invocations, {} saved tokens, {} failures\n",
        analysis.database,
        format_count(analysis.overall.invocations),
        format_count(analysis.overall.saved.tokens),
        format_count(analysis.overall.failures),
    ));
    output.push_str("\nRecommended focus areas:\n");
    for (index, recommendation) in recommendations.iter().enumerate() {
        output.push_str(&format!(
            "{}. {}\n   Command: {}\n   Evidence: {}\n   Action: {}\n",
            index + 1,
            recommendation.title,
            recommendation.command,
            recommendation.evidence,
            recommendation.action,
        ));
    }
    Ok(ProxyOutcome::success(output.trim_end().to_string()))
}

pub fn run_failures(
    limit: usize,
    level: CommandLevel,
    filter: insights::CommandFilter<'_>,
) -> Result<ProxyOutcome> {
    let coverage = load_failure_coverage(level, filter)?;
    let mut output = String::from("cx insights: failures\n");
    output.push_str(&format!(
        "Database: {}\nLevel: {}\nArtifacts: ~/.cx/cache/failures\n",
        insights::insights_database_path()?.display(),
        command_level_label(level),
    ));
    output.push_str(&format_filter_line(filter));
    if coverage.summary.failed_invocations == 0 {
        output.push_str("No failed invocations recorded.");
        return Ok(ProxyOutcome::success(output));
    }

    output.push('\n');
    output.push_str(&format_failure_focus(
        &coverage.rows[..coverage.rows.len().min(limit.clamp(1, 100))],
    ));
    Ok(ProxyOutcome::success(output))
}

pub fn run_export(
    format: ExportFormat,
    limit: usize,
    filter: insights::CommandFilter<'_>,
) -> Result<ProxyOutcome> {
    let evidence = ExportEvidence::load(limit, filter)?;
    let output = match format {
        ExportFormat::Json => format_export_json(&evidence)?,
        ExportFormat::Csv => format_export_csv(&evidence),
    };
    Ok(ProxyOutcome::success(output))
}

#[cfg(test)]
mod tests;
