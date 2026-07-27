use anyhow::Result;

use crate::support::{
    insights::{
        self, CommandFilter, CommandLevel, CommandOpportunityInsight, CommandReportTotalInsight,
        CommandTotalInsight, DailyInsight, FailureArtifactInsight, InsightSettingsSnapshot,
        InvocationInsight, OverallInsight, TextMetrics,
    },
    runner::ProxyOutcome,
};

use super::data::{ExportEvidence, FilterSummary, Recommendation};
use super::distribution_view::savings_distribution_json;
use super::failure_coverage::FailureFocus;
use super::format_utils::{div_floor, ratio_value, signed_delta};
use super::presentation::{
    format_impact_bullets, format_presentation_headlines, PresentationMetrics,
    DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
};
use super::EXPORT_SCHEMA_VERSION;

const DASHBOARD_SCHEMA_NAME: &str = "cx-insights-dashboard";
const DASHBOARD_SCHEMA_VERSION: u64 = 12;

pub fn run(limit: usize, filter: CommandFilter<'_>) -> Result<ProxyOutcome> {
    let evidence = DashboardEvidence::load(limit.clamp(1, 100), filter)?;
    let output = serde_json::to_string_pretty(&evidence.to_json())?;
    Ok(ProxyOutcome::success(output))
}

struct DashboardEvidence {
    export: ExportEvidence,
    report_roots: Vec<CommandReportTotalInsight>,
    expanded_invocations: Vec<InvocationInsight>,
    settings: InsightSettingsSnapshot,
}

impl DashboardEvidence {
    fn load(limit: usize, filter: CommandFilter<'_>) -> Result<Self> {
        Ok(Self {
            export: ExportEvidence::load(limit, filter)?,
            report_roots: insights::command_report_totals_at_level(
                CommandLevel::Root,
                limit,
                filter,
            )?,
            expanded_invocations: insights::expansion_invocations_filtered(limit, filter)?,
            settings: insights::insight_settings_snapshot()?,
        })
    }

    fn to_json(&self) -> serde_json::Value {
        let export = &self.export;
        let snapshot = &export.snapshot;
        serde_json::json!({
            "schema_name": DASHBOARD_SCHEMA_NAME,
            "schema_version": DASHBOARD_SCHEMA_VERSION,
            "source_export_schema_version": EXPORT_SCHEMA_VERSION,
            "generated_at_ms": export.generated_at_ms,
            "database": snapshot.database,
            "limit": export.limit,
            "contract": contract_json(export.limit),
            "provenance": provenance_json(self),
            "filter": filter_json(&export.filter),
            "settings": settings_json(&self.settings),
            "capabilities": capabilities_json(&self.settings),
            "empty_state": empty_state_json(self),
            "summary": overall_json(&snapshot.overall),
            "savings_distribution": savings_distribution_json(&snapshot.savings_distribution),
            "quality_report_status": super::report_view::status_summary_json(
                &export.command_report_status,
            ),
            "cards": dashboard_cards_json(self),
            "charts": {
                "daily_savings": snapshot.daily_totals.iter().map(daily_json).collect::<Vec<_>>(),
                "root_savings": snapshot.top_roots.iter().map(command_chart_json).collect::<Vec<_>>(),
            },
            "tables": {
                "command_roots": snapshot.top_roots.iter().map(|row| command_total_json(row, "root")).collect::<Vec<_>>(),
                "command_families": snapshot.top_commands.iter().map(|row| command_total_json(row, "family")).collect::<Vec<_>>(),
                "largest_invocations": snapshot.largest_invocations.iter().map(invocation_json).collect::<Vec<_>>(),
                "recent_invocations": snapshot.recent_invocations.iter().map(invocation_json).collect::<Vec<_>>(),
                "expanded_invocations": self.expanded_invocations.iter().map(invocation_json).collect::<Vec<_>>(),
                "quality_report_roots": self.report_roots.iter().map(|row| report_total_json(row, "root")).collect::<Vec<_>>(),
                "quality_report_families": export.command_report_totals.iter().map(|row| report_total_json(row, "family")).collect::<Vec<_>>(),
                "recent_quality_reports": export.recent_command_reports.iter().map(super::report_view::report_json).collect::<Vec<_>>(),
                "recent_failure_artifacts": export.recent_failure_artifacts.iter().map(failure_artifact_json).collect::<Vec<_>>(),
                "failure_focus": export.failure_focus.iter().map(failure_focus_json).collect::<Vec<_>>(),
                "passthrough_opportunities": export.passthrough_opportunities.iter().map(opportunity_json).collect::<Vec<_>>(),
                "routing_decision_totals": export.routing_decision_totals.iter().map(super::routing_view::routing_total_json).collect::<Vec<_>>(),
                "recent_routing_decisions": export.recent_routing_decisions.iter().map(super::routing_view::routing_decision_json).collect::<Vec<_>>(),
            },
            "recommendations": export.recommendations.iter().enumerate().map(|(index, item)| recommendation_json(index, item)).collect::<Vec<_>>(),
            "presentation": presentation_json(export),
            "health": health_json(self),
        })
    }
}

fn contract_json(limit: usize) -> serde_json::Value {
    serde_json::json!({
        "bounded": true,
        "row_limit_per_collection": limit,
        "metric_fields": ["bytes", "chars", "lines", "tokens"],
        "metric_semantics": {
            "raw": "observed output before CX projection",
            "emitted": "output returned by CX",
            "saved": "positive raw minus emitted delta",
            "expanded": "positive emitted minus raw delta",
            "tokens_are_estimates": true,
        },
        "ratio_semantics": {
            "savings_ratio": "fraction from 0.0 to 1.0",
            "compression_ratio": "emitted characters divided by raw characters",
            "rates": "fraction from 0.0 to 1.0",
        },
        "distribution_semantics": {
            "percentile_method": "nearest-rank",
            "all_invocations": "all matching command_invocations rows, including zero savings",
            "saving_invocations": "matching rows with saved_tokens greater than zero",
            "top_10": "ten largest matching saved_tokens values across the full filtered dataset",
            "independent_of_row_limit": true,
        },
        "timestamp_semantics": "unix epoch milliseconds",
        "filters": ["command_root", "command_family"],
        "refresh_model": "on-demand CLI snapshot",
    })
}

fn provenance_json(evidence: &DashboardEvidence) -> serde_json::Value {
    serde_json::json!({
        "database_exists": evidence.settings.database_exists,
        "database": evidence.settings.database,
        "authoritative_reader": "cx insights dashboard",
        "source_tables": [
            "command_invocations",
            "command_reports",
            "command_report_dispositions",
            "command_report_evidence",
            "failure_artifacts",
            "command_opportunities",
            "command_routing_decisions",
            "settings",
        ],
        "failure_artifacts": "~/.cx/cache/failures/<tool>",
    })
}

fn settings_json(snapshot: &InsightSettingsSnapshot) -> serde_json::Value {
    let values = snapshot
        .rows
        .iter()
        .map(|row| (row.key.clone(), serde_json::json!(row.value)))
        .collect::<serde_json::Map<_, _>>();
    let definitions = snapshot
        .rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "key": row.key,
                "value": row.value,
                "description": row.description,
                "sensitivity": setting_sensitivity(&row.key),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "database_exists": snapshot.database_exists,
        "values": values,
        "definitions": definitions,
    })
}

fn setting_sensitivity(key: &str) -> &'static str {
    match key {
        "record_command_text" | "record_sources" => "command-metadata",
        "record_failure_responses" | "record_response_previews" => "response-content",
        "passthrough_unsupported_commands" => "execution",
        _ => "metrics",
    }
}

fn capabilities_json(snapshot: &InsightSettingsSnapshot) -> serde_json::Value {
    serde_json::json!({
        "recording_enabled": setting_value(snapshot, "record_invocations"),
        "command_text_recording_enabled": setting_value(snapshot, "record_command_text"),
        "command_shape_recording_enabled": setting_value(snapshot, "record_command_shape"),
        "source_recording_enabled": setting_value(snapshot, "record_sources"),
        "failure_detail_recording_enabled": setting_value(snapshot, "record_failures"),
        "failure_response_recording_enabled": setting_value(snapshot, "record_failure_responses"),
        "unsupported_passthrough_enabled": setting_value(snapshot, "passthrough_unsupported_commands"),
        "command_optimizations_enabled": setting_value(snapshot, "command_optimizations"),
        "document_search_compaction_enabled": setting_value(snapshot, "compact_document_search_results"),
        "sections": [
            "overview",
            "command_roots",
            "command_families",
            "recent_activity",
            "largest_saves",
            "savings_distribution",
            "expansions",
            "reliability",
            "quality_reports",
            "opportunities",
            "routing_decisions",
            "recommendations",
            "presentation",
            "settings",
        ],
    })
}

fn setting_value(snapshot: &InsightSettingsSnapshot, key: &str) -> bool {
    snapshot
        .rows
        .iter()
        .find(|row| row.key == key)
        .is_some_and(|row| row.value)
}

fn empty_state_json(evidence: &DashboardEvidence) -> serde_json::Value {
    let overall = &evidence.export.snapshot.overall;
    serde_json::json!({
        "database_missing": !evidence.settings.database_exists,
        "recording_disabled": !setting_value(&evidence.settings, "record_invocations"),
        "no_invocations": overall.invocations == 0,
        "no_quality_reports": report_count(evidence) == 0,
        "no_open_quality_reports": evidence.export.command_report_status.open == 0,
        "no_failures": overall.failures == 0,
        "no_opportunities": evidence.export.passthrough_opportunities.is_empty(),
        "no_routing_rejections": evidence.export.routing_summary.rejections == 0,
        "command_text_recording_disabled": overall.invocations > 0
            && !setting_value(&evidence.settings, "record_command_text"),
        "source_recording_disabled": overall.invocations > 0
            && !setting_value(&evidence.settings, "record_sources"),
    })
}

fn filter_json(filter: &FilterSummary) -> serde_json::Value {
    serde_json::json!({
        "active": !filter.is_empty(),
        "command_root": filter.command_root.as_deref(),
        "command_family": filter.command.as_deref(),
    })
}

fn overall_json(overall: &OverallInsight) -> serde_json::Value {
    serde_json::json!({
        "invocations": overall.invocations,
        "failures": overall.failures,
        "expansions": overall.expansions,
        "raw": metrics_json(&overall.raw),
        "emitted": metrics_json(&overall.emitted),
        "saved": metrics_json(&overall.saved),
        "expanded": metrics_json(&overall.expanded),
        "net_token_delta": signed_delta(overall.emitted.tokens, overall.raw.tokens),
        "savings_ratio": ratio_value(overall.saved.chars, overall.raw.chars),
        "compression_ratio": ratio_value(overall.emitted.chars, overall.raw.chars),
        "failure_rate": ratio_value(overall.failures, overall.invocations),
        "expansion_rate": ratio_value(overall.expansions, overall.invocations),
    })
}

fn dashboard_cards_json(evidence: &DashboardEvidence) -> Vec<serde_json::Value> {
    let mut cards = primary_dashboard_cards_json(evidence);
    cards.extend(secondary_dashboard_cards_json(evidence));
    cards
}

fn primary_dashboard_cards_json(evidence: &DashboardEvidence) -> Vec<serde_json::Value> {
    let overall = &evidence.export.snapshot.overall;
    vec![
        card_json(
            "invocations",
            "Invocations",
            overall.invocations,
            "commands",
            "neutral",
        ),
        card_json(
            "saved_tokens",
            "Saved tokens",
            overall.saved.tokens,
            "estimated tokens",
            "positive",
        ),
        card_json(
            "saved_lines",
            "Saved lines",
            overall.saved.lines,
            "lines",
            "positive",
        ),
        card_json(
            "failures",
            "Failures",
            overall.failures,
            "commands",
            failure_tone(overall),
        ),
        card_json(
            "open_quality_reports",
            "Open quality reports",
            evidence.export.command_report_status.open,
            "reports",
            quality_report_tone(evidence),
        ),
        card_json(
            "denied_quality_reports",
            "Denied quality reports",
            evidence.export.command_report_status.denied,
            "reports",
            "neutral",
        ),
        card_json(
            "routing_rejections",
            "Routing rejections",
            evidence.export.routing_summary.rejections,
            "commands",
            routing_tone(evidence),
        ),
        card_json(
            "artifact_output_gaps",
            "Artifact output gaps",
            missing_artifact_count(evidence),
            "families",
            health_tone(evidence),
        ),
    ]
}

fn secondary_dashboard_cards_json(evidence: &DashboardEvidence) -> Vec<serde_json::Value> {
    let overall = &evidence.export.snapshot.overall;
    vec![
        card_json(
            "expansions",
            "Expanded invocations",
            overall.expansions,
            "commands",
            expansion_tone(overall),
        ),
        card_json(
            "expanded_tokens",
            "Expanded tokens",
            overall.expanded.tokens,
            "estimated tokens",
            expansion_tone(overall),
        ),
        card_json(
            "potential_tokens",
            "Potential tokens",
            potential_saved_tokens(evidence),
            "estimated tokens",
            opportunity_tone(evidence),
        ),
        card_json(
            "median_saving_call_tokens",
            "Median saving call",
            evidence
                .export
                .snapshot
                .savings_distribution
                .saving_p50_saved_tokens,
            "estimated tokens",
            "positive",
        ),
        card_json(
            "saved_tokens_excluding_top_10",
            "Saved outside top 10",
            evidence
                .export
                .snapshot
                .savings_distribution
                .saved_tokens_excluding_top_ten(),
            "estimated tokens",
            "positive",
        ),
    ]
}

fn card_json(id: &str, label: &str, value: u64, unit: &str, tone: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "label": label,
        "value": value,
        "unit": unit,
        "tone": tone,
    })
}

fn command_total_json(total: &CommandTotalInsight, level: &str) -> serde_json::Value {
    serde_json::json!({
        "id": format!("{level}:{}", total.command),
        "group_level": level,
        "command_root": insights::command_root(&total.command),
        "command_family": total.command,
        "invocations": total.invocations,
        "failures": total.failures,
        "expansions": total.expansions,
        "raw": metrics_json(&total.raw),
        "emitted": metrics_json(&total.emitted),
        "saved": metrics_json(&total.saved),
        "expanded": metrics_json(&total.expanded),
        "net_token_delta": signed_delta(total.emitted.tokens, total.raw.tokens),
        "average_saved_tokens": div_floor(total.saved.tokens, total.invocations),
        "best_saved_tokens": total.best_saved_tokens,
        "best_expanded_tokens": total.best_expanded_tokens,
        "savings_ratio": ratio_value(total.saved.chars, total.raw.chars),
        "compression_ratio": ratio_value(total.emitted.chars, total.raw.chars),
        "failure_rate": ratio_value(total.failures, total.invocations),
    })
}

fn command_chart_json(total: &CommandTotalInsight) -> serde_json::Value {
    serde_json::json!({
        "command_root": insights::command_root(&total.command),
        "command_family": total.command,
        "saved_tokens": total.saved.tokens,
        "expanded_tokens": total.expanded.tokens,
        "net_token_delta": signed_delta(total.emitted.tokens, total.raw.tokens),
        "invocations": total.invocations,
        "failures": total.failures,
    })
}

fn invocation_json(invocation: &InvocationInsight) -> serde_json::Value {
    serde_json::json!({
        "id": invocation.id,
        "occurred_at_ms": invocation.occurred_at_ms,
        "process": invocation.process,
        "command_root": insights::command_root(&invocation.command_family),
        "command_family": invocation.command_family,
        "command": invocation.command,
        "argv": argv_json_value(&invocation.argv_json),
        "command_shape": invocation.command_shape,
        "command_shape_hash": invocation.command_shape_hash,
        "source": invocation.source,
        "thread_id": invocation.thread_id,
        "plan_title": invocation.plan_title,
        "plan_project_id": invocation.plan_project_id,
        "plan_folder_id": invocation.plan_folder_id,
        "cwd": invocation.cwd,
        "git_root": invocation.git_root,
        "binary_version": invocation.binary_version,
        "binary_revision": invocation.binary_revision,
        "binary_fingerprint": invocation.binary_fingerprint,
        "emitted_response_preview": invocation.emitted_response_preview,
        "raw_response_preview": invocation.raw_response_preview,
        "exit_code": invocation.exit_code,
        "raw": metrics_json(&invocation.raw),
        "emitted": metrics_json(&invocation.emitted),
        "saved": metrics_json(&invocation.saved),
        "expanded": metrics_json(&invocation.expanded),
        "expansion_reason": invocation.expansion_reason,
        "net_token_delta": signed_delta(invocation.emitted.tokens, invocation.raw.tokens),
        "savings_ratio": invocation.savings_ratio,
        "compression_ratio": invocation.compression_ratio,
    })
}

fn failure_artifact_json(artifact: &FailureArtifactInsight) -> serde_json::Value {
    serde_json::json!({
        "id": artifact.id,
        "created_at_ms": artifact.created_at_ms,
        "display_path": artifact.display_path,
        "tool_name": artifact.tool_name,
        "compression": artifact.compression,
        "stdout_bytes": artifact.stdout_bytes,
        "stderr_bytes": artifact.stderr_bytes,
        "original_bytes": artifact.original_bytes,
        "stored_bytes": artifact.stored_bytes,
        "invocation_id": artifact.invocation_id,
        "report_id": artifact.report_id,
        "exit_code": artifact.exit_code,
        "binary_revision": artifact.binary_revision,
        "binary_fingerprint": artifact.binary_fingerprint,
    })
}

fn daily_json(day: &DailyInsight) -> serde_json::Value {
    serde_json::json!({
        "day": day.day,
        "saved": metrics_json(&day.saved),
        "expanded": metrics_json(&day.expanded),
        "net_token_delta": signed_delta(day.expanded.tokens, day.saved.tokens),
        "invocations": day.invocations,
        "failures": day.failures,
        "expansions": day.expansions,
        "failure_rate": ratio_value(day.failures, day.invocations),
        "expansion_rate": ratio_value(day.expansions, day.invocations),
    })
}

fn report_total_json(total: &CommandReportTotalInsight, level: &str) -> serde_json::Value {
    serde_json::json!({
        "id": format!("{level}:{}", total.command),
        "group_level": level,
        "command_root": insights::command_root(&total.command),
        "command_family": total.command,
        "reports": total.reports,
        "open": total.open,
        "resolved": total.resolved,
        "native_parity": total.native_parity,
        "not_reproducible": total.not_reproducible,
        "denied": total.denied,
        "other": total.other,
    })
}

fn failure_focus_json(focus: &FailureFocus) -> serde_json::Value {
    serde_json::json!({
        "command_root": insights::command_root(&focus.total.command),
        "command_family": focus.total.command,
        "failures": focus.total.failures,
        "invocations": focus.total.invocations,
        "saved": metrics_json(&focus.total.saved),
        "artifact_tool": focus.artifact_summary.tool_name,
        "artifact_directory": focus.artifact_summary.display_dir,
        "artifact_count": focus.artifact_summary.count,
        "latest_artifact": focus.artifact_summary.latest_display_path,
        "detail_rows": focus.coverage.detail_rows,
        "linked_detail_rows": focus.coverage.linked_detail_rows,
        "linked_invocations": focus.coverage.linked_invocations,
        "orphan_detail_rows": focus.coverage.orphan_detail_rows,
        "unknown_invocations": focus.coverage.unknown_invocations,
        "output_bearing_detail_rows": focus.coverage.output_bearing_detail_rows,
        "silent_detail_rows": focus.coverage.silent_detail_rows,
        "artifact_linked_detail_rows": focus.coverage.artifact_linked_detail_rows,
        "output_gap_detail_rows": focus.coverage.output_gap_detail_rows,
        "response_evidence_available": focus.coverage.response_evidence_available,
        "artifact_reference_available": focus.coverage.artifact_reference_available,
        "has_output_gap": focus.has_output_gap(),
        "coverage_unknown": focus.coverage_unknown(),
        "linked_but_pruned": focus.linked_but_pruned(),
    })
}

fn opportunity_json(item: &CommandOpportunityInsight) -> serde_json::Value {
    serde_json::json!({
        "id": format!("{}:{}:{}", item.process, item.command_family, item.strategy),
        "process": item.process,
        "command_root": insights::command_root(&item.command_family),
        "command_family": item.command_family,
        "strategy": item.strategy,
        "confidence": item.confidence.as_str(),
        "samples": item.samples,
        "latest_at_ms": item.latest_at_ms,
        "raw": metrics_json(&item.raw),
        "projected": metrics_json(&item.projected),
        "potential_saved": metrics_json(&item.potential_saved),
        "potential_savings_ratio": ratio_value(item.potential_saved.chars, item.raw.chars),
        "best_potential_saved_tokens": item.best_potential_saved_tokens,
        "estimate": true,
    })
}

fn recommendation_json(index: usize, item: &Recommendation) -> serde_json::Value {
    serde_json::json!({
        "id": format!("recommendation:{}", index + 1),
        "title": item.title,
        "command_family": item.command,
        "evidence": item.evidence,
        "action": item.action,
    })
}

fn presentation_json(export: &ExportEvidence) -> serde_json::Value {
    let metrics = PresentationMetrics::from_snapshot(
        &export.snapshot,
        DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
    );
    serde_json::json!({
        "metrics": {
            "invocations": metrics.invocations,
            "failures": metrics.failures,
            "expansions": metrics.expansions,
            "failure_rate": metrics.failure_rate,
            "raw": metrics_json(&metrics.raw),
            "emitted": metrics_json(&metrics.emitted),
            "saved": metrics_json(&metrics.saved),
            "expanded": metrics_json(&metrics.expanded),
            "net_token_delta": metrics.net_token_delta,
            "savings_ratio": metrics.savings_ratio,
            "average_saved_tokens": metrics.average_saved_tokens,
            "savings_distribution": savings_distribution_json(&metrics.savings_distribution),
            "context_window_tokens": metrics.context_window_tokens,
            "context_windows_saved": metrics.context_windows_saved,
        },
        "headlines": lines_json(&format_presentation_headlines(&export.snapshot)),
        "speaker_bullets": lines_json(&format_impact_bullets(
            &export.snapshot,
            DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
        )),
    })
}

fn lines_json(value: &str) -> Vec<&str> {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn metrics_json(metrics: &TextMetrics) -> serde_json::Value {
    serde_json::json!({
        "bytes": metrics.bytes,
        "chars": metrics.chars,
        "lines": metrics.lines,
        "tokens": metrics.tokens,
    })
}

fn argv_json_value(argv_json: &str) -> serde_json::Value {
    serde_json::from_str(argv_json).unwrap_or_else(|_| serde_json::json!([]))
}

fn report_count(evidence: &DashboardEvidence) -> u64 {
    evidence.export.command_report_status.total
}

fn missing_artifact_count(evidence: &DashboardEvidence) -> u64 {
    evidence.export.failure_coverage.groups_with_output_gaps
}

fn artifact_backed_count(evidence: &DashboardEvidence) -> u64 {
    evidence
        .export
        .failure_coverage
        .groups_with_retained_artifacts
}

fn potential_saved_tokens(evidence: &DashboardEvidence) -> u64 {
    evidence
        .export
        .passthrough_opportunities
        .iter()
        .map(|item| item.potential_saved.tokens)
        .sum()
}

fn opportunity_samples(evidence: &DashboardEvidence) -> u64 {
    evidence
        .export
        .passthrough_opportunities
        .iter()
        .map(|item| item.samples)
        .sum()
}

fn health_json(evidence: &DashboardEvidence) -> serde_json::Value {
    let overall = &evidence.export.snapshot.overall;
    let coverage = evidence.export.failure_coverage;
    serde_json::json!({
        "failed_invocations": coverage.failed_invocations,
        "failure_rate": ratio_value(overall.failures, overall.invocations),
        "quality_reports": report_count(evidence),
        "open_quality_reports": evidence.export.command_report_status.open,
        "closed_quality_reports": evidence.export.command_report_status.closed(),
        "resolved_quality_reports": evidence.export.command_report_status.resolved,
        "native_parity_reports": evidence.export.command_report_status.native_parity,
        "not_reproducible_reports": evidence.export.command_report_status.not_reproducible,
        "denied_quality_reports": evidence.export.command_report_status.denied,
        "quality_report_denial_reasons": denial_reason_health_json(evidence),
        "failing_command_families": coverage.failing_groups,
        "detailed_failure_rows": coverage.detail_rows,
        "linked_failure_detail_rows": coverage.linked_detail_rows,
        "orphan_failure_detail_rows": coverage.orphan_detail_rows,
        "unknown_failure_invocations": coverage.unknown_invocations,
        "output_bearing_failure_rows": coverage.output_bearing_detail_rows,
        "silent_failure_rows": coverage.silent_detail_rows,
        "artifact_linked_failure_rows": coverage.artifact_linked_detail_rows,
        "output_without_artifact_rows": coverage.output_gap_detail_rows,
        "missing_artifact_risks": coverage.groups_with_output_gaps,
        "families_with_output_gaps": coverage.groups_with_output_gaps,
        "families_with_unknown_coverage": coverage.groups_with_unknown_coverage,
        "artifact_backed_families": artifact_backed_count(evidence),
        "families_with_retained_artifacts": coverage.groups_with_retained_artifacts,
        "families_with_linked_but_pruned_artifacts": coverage.groups_with_linked_but_pruned_artifacts,
        "expanded_invocations": overall.expansions,
        "expansion_rate": ratio_value(overall.expansions, overall.invocations),
        "expanded_tokens": overall.expanded.tokens,
        "net_token_delta": signed_delta(overall.emitted.tokens, overall.raw.tokens),
        "opportunity_samples": opportunity_samples(evidence),
        "potential_saved_tokens": potential_saved_tokens(evidence),
        "routing_rejections": evidence.export.routing_summary.rejections,
        "routing_passthrough_eligible": evidence.export.routing_summary.passthrough_eligible,
        "routing_passthrough_disabled": evidence.export.routing_summary.passthrough_disabled,
        "routing_cx_owned_errors": evidence.export.routing_summary.cx_owned_errors,
        "latest_routing_rejection_at_ms": evidence.export.routing_summary.latest_at_ms,
        "routing_summary": super::routing_view::routing_summary_json(
            &evidence.export.routing_summary,
        ),
    })
}

fn denial_reason_health_json(evidence: &DashboardEvidence) -> serde_json::Value {
    let summary = evidence.export.command_report_denial_reasons;
    serde_json::json!({
        "total": summary.total,
        "duplicate": summary.duplicate,
        "insufficient_evidence": summary.insufficient_evidence,
        "invalid": summary.invalid,
        "obsolete": summary.obsolete,
        "unsupported": summary.unsupported,
        "low_value": summary.low_value,
        "other": summary.other,
    })
}

fn failure_tone(overall: &OverallInsight) -> &'static str {
    if overall.failures > 0 {
        "warning"
    } else {
        "neutral"
    }
}

fn health_tone(evidence: &DashboardEvidence) -> &'static str {
    if missing_artifact_count(evidence) > 0 {
        "warning"
    } else {
        "positive"
    }
}

fn expansion_tone(overall: &OverallInsight) -> &'static str {
    if overall.expansions > 0 {
        "warning"
    } else {
        "neutral"
    }
}

fn opportunity_tone(evidence: &DashboardEvidence) -> &'static str {
    if evidence.export.passthrough_opportunities.is_empty() {
        "neutral"
    } else {
        "positive"
    }
}

fn routing_tone(evidence: &DashboardEvidence) -> &'static str {
    if evidence.export.routing_summary.rejections > 0 {
        "warning"
    } else {
        "neutral"
    }
}

fn quality_report_tone(evidence: &DashboardEvidence) -> &'static str {
    if evidence.export.command_report_status.open > 0 {
        "warning"
    } else {
        "positive"
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::support::insights::{
        record_command_opportunity, record_command_report, record_invocation,
        record_routing_rejection, set_insight_setting, CommandOpportunityRecord,
        CommandReportRecord, InvocationRecord, OutputObservation, RoutingDecisionRecord,
    };

    #[test]
    fn run_dashboard_renders_complete_ui_contract() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".cx/cache/failures/grep")).unwrap();
        crate::support::test_support::with_env_vars(
            &[
                ("HOME", Some(home.to_string_lossy().as_ref())),
                (
                    "CX_INSIGHTS_DB_PATH",
                    Some(db_path.to_string_lossy().as_ref()),
                ),
                ("CX_DISABLE_INSIGHTS", None),
            ],
            || {
                seed_dashboard_fixture();
                set_insight_setting("record_invocations", "true").unwrap();
                set_insight_setting("record_command_text", "true").unwrap();
                set_insight_setting("record_sources", "true").unwrap();

                let output = run(5, CommandFilter::default()).unwrap();
                let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();

                assert_complete_dashboard_contract(&value);
            },
        );
    }

    fn assert_complete_dashboard_contract(value: &serde_json::Value) {
        assert_dashboard_metadata_contract(value);
        assert_dashboard_distribution_contract(value);
        assert_dashboard_tables_contract(value);
        assert_dashboard_presentation_contract(value);
        assert_routing_dashboard_contract(value);
    }

    fn assert_dashboard_metadata_contract(value: &serde_json::Value) {
        assert_eq!(value["schema_name"], DASHBOARD_SCHEMA_NAME);
        assert_eq!(value["schema_version"], DASHBOARD_SCHEMA_VERSION);
        assert_eq!(value["source_export_schema_version"], EXPORT_SCHEMA_VERSION);
        assert_eq!(value["contract"]["metric_fields"][3], "tokens");
        assert_eq!(
            value["contract"]["metric_semantics"]["tokens_are_estimates"],
            true
        );
        assert_eq!(
            value["contract"]["distribution_semantics"]["percentile_method"],
            "nearest-rank"
        );
        assert_eq!(
            value["contract"]["distribution_semantics"]["independent_of_row_limit"],
            true
        );
        assert_eq!(value["settings"]["values"]["record_invocations"], true);
        assert_eq!(
            value["capabilities"]["command_text_recording_enabled"],
            true
        );
        assert_eq!(value["empty_state"]["no_invocations"], false);
        assert_eq!(value["summary"]["invocations"], 2);
        assert!(value["summary"]["raw"]["tokens"].as_u64().unwrap() > 0);
    }

    fn assert_dashboard_distribution_contract(value: &serde_json::Value) {
        assert_eq!(value["cards"][1]["id"], "saved_tokens");
        assert_eq!(value["cards"][5]["id"], "denied_quality_reports");
        assert_eq!(value["cards"][6]["id"], "routing_rejections");
        assert_eq!(value["cards"][10]["id"], "potential_tokens");
        assert_eq!(value["cards"][11]["id"], "median_saving_call_tokens");
        assert_eq!(value["cards"][12]["id"], "saved_tokens_excluding_top_10");
        assert_eq!(value["savings_distribution"]["invocations"], 2);
        assert_eq!(
            value["savings_distribution"]["concentration"]["top_10_share"],
            1.0
        );
    }

    fn assert_dashboard_tables_contract(value: &serde_json::Value) {
        assert_eq!(value["tables"]["command_roots"][0]["group_level"], "root");
        assert_eq!(
            value["tables"]["command_families"][0]["group_level"],
            "family"
        );
        assert!(value["tables"]["recent_invocations"][0]["raw"]["tokens"].is_u64());
        assert!(value["tables"]["recent_invocations"][0]["emitted"]["tokens"].is_u64());
        assert_eq!(
            value["tables"]["passthrough_opportunities"][0]["estimate"],
            true
        );
        assert_eq!(
            value["tables"]["recent_quality_reports"][0]["issue_kind"],
            "suspicious_output"
        );
        assert!(value["recommendations"].as_array().unwrap().len() >= 2);
    }

    fn assert_dashboard_presentation_contract(value: &serde_json::Value) {
        assert!(value["presentation"]["metrics"]["context_windows_saved"].is_f64());
        assert_eq!(
            value["presentation"]["metrics"]["savings_distribution"]["saving_invocations"],
            2
        );
        assert_eq!(value["health"]["quality_reports"], 1);
        assert_eq!(value["health"]["opportunity_samples"], 1);
    }

    #[test]
    fn run_dashboard_exposes_expansion_drilldown() {
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
                let raw = OutputObservation::from_text("git status", "")
                    .with_expansion_reason("status-summary");
                record_invocation(&InvocationRecord {
                    command: "git status",
                    exit_code: 0,
                    raw: Some(&raw),
                    emitted: TextMetrics::from_text("Clean working tree"),
                })
                .unwrap();

                let output = run(5, CommandFilter::default()).unwrap();
                let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
                assert_eq!(value["health"]["expanded_invocations"], 1);
                assert!(value["health"]["expanded_tokens"].as_u64().unwrap() > 0);
                assert!(value["health"]["net_token_delta"].as_i64().unwrap() > 0);
                assert_eq!(
                    value["tables"]["expanded_invocations"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(
                    value["tables"]["expanded_invocations"][0]["expansion_reason"],
                    "status-summary"
                );
                assert_eq!(
                    value["tables"]["expanded_invocations"][0]["raw"]["tokens"],
                    0
                );
                assert!(
                    value["tables"]["expanded_invocations"][0]["emitted"]["tokens"]
                        .as_u64()
                        .unwrap()
                        > 0
                );
            },
        );
    }

    #[test]
    fn run_dashboard_keeps_missing_database_and_report_only_states_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let missing_db = temp.path().join("missing.sqlite");
        crate::support::test_support::with_env_vars(
            &[
                (
                    "CX_INSIGHTS_DB_PATH",
                    Some(missing_db.to_string_lossy().as_ref()),
                ),
                ("CX_DISABLE_INSIGHTS", None),
            ],
            || {
                let output = run(3, CommandFilter::default()).unwrap();
                let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
                assert_eq!(value["empty_state"]["database_missing"], true);
                assert_eq!(value["empty_state"]["no_invocations"], true);
                assert!(!missing_db.exists());

                record_command_report(&CommandReportRecord {
                    command: "cx git diff -- src",
                    command_family: "git diff",
                    command_shape: "",
                    command_shape_hash: "",
                    issue_kind: "incorrect_summary",
                    note: "test report",
                })
                .unwrap();
                let output = run(
                    3,
                    CommandFilter {
                        command_root: Some("git"),
                        command: None,
                    },
                )
                .unwrap();
                let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
                assert_eq!(value["filter"]["active"], true);
                assert_eq!(value["empty_state"]["database_missing"], false);
                assert_eq!(value["empty_state"]["no_invocations"], true);
                assert_eq!(value["empty_state"]["no_quality_reports"], false);
                assert_eq!(
                    value["tables"]["quality_report_roots"][0]["command_root"],
                    "git"
                );
            },
        );
    }

    fn seed_dashboard_fixture() {
        let raw = OutputObservation::from_text("git diff", &"one\n".repeat(24));
        record_invocation(&InvocationRecord {
            command: "git diff",
            exit_code: 0,
            raw: Some(&raw),
            emitted: TextMetrics::from_text("one\n"),
        })
        .unwrap();
        let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(12));
        record_invocation(&InvocationRecord {
            command: "grep",
            exit_code: 2,
            raw: Some(&grep_raw),
            emitted: TextMetrics::from_text("match\n"),
        })
        .unwrap();
        record_command_report(&CommandReportRecord {
            command: "cx grep route|path src",
            command_family: "grep",
            command_shape: "",
            command_shape_hash: "",
            issue_kind: "suspicious_output",
            note: "bare alternation returned zero matches",
        })
        .unwrap();
        record_command_opportunity(&CommandOpportunityRecord {
            process: "passthrough",
            command_family: "passthrough ps",
            command: "ps aux",
            source: "ps",
            strategy: "bounded-head-tail",
            confidence: insights::OpportunityConfidence::Low,
            raw: TextMetrics {
                bytes: 20_000,
                chars: 20_000,
                lines: 300,
                tokens: 5_000,
            },
            projected: TextMetrics {
                bytes: 2_000,
                chars: 2_000,
                lines: 40,
                tokens: 500,
            },
        })
        .unwrap();
        record_routing_rejection(&RoutingDecisionRecord {
            args: &[
                "cx".into(),
                "--".into(),
                "git".into(),
                "branch".into(),
                "--show-current".into(),
            ],
            reason: "passthrough-disabled",
            error_kind: "invalid-subcommand",
            explicit_auto: true,
            passthrough_eligible: true,
            passthrough_enabled: false,
        })
        .unwrap();
    }

    fn assert_routing_dashboard_contract(value: &serde_json::Value) {
        assert_eq!(value["health"]["routing_summary"]["rejections"], 1);
        assert_eq!(
            value["tables"]["recent_routing_decisions"][0]["reason"],
            "passthrough-disabled"
        );
    }
}
