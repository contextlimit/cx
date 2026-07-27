use anyhow::Result;

use crate::support::insights::{
    CommandReportTotalInsight, CommandTotalInsight, DailyInsight, FailureArtifactInsight,
    InvocationInsight, OverallInsight,
};

use super::data::{ExportEvidence, FilterSummary, Recommendation};
use super::distribution_view::savings_distribution_json;
use super::export::{metrics_json, EXPORT_SCHEMA_NAME, EXPORT_SCHEMA_VERSION};
use super::failure_coverage::FailureFocus;
use super::format_utils::{div_floor, ratio_value, signed_delta};
use super::opportunity_view;
use super::presentation::{
    format_impact_bullets, format_presentation_demo_commands, format_presentation_headlines,
    format_presentation_slide_outline, PresentationMetrics,
    DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
};
use super::routing_view;

pub(super) fn format_export_json(evidence: &ExportEvidence) -> Result<String> {
    let snapshot = &evidence.snapshot;
    let value = serde_json::json!({
        "schema_name": EXPORT_SCHEMA_NAME,
        "schema_version": EXPORT_SCHEMA_VERSION,
        "generated_at_ms": evidence.generated_at_ms,
        "limit": evidence.limit,
        "database": snapshot.database,
        "filter": filter_json(&evidence.filter),
        "no_data": snapshot.no_data(),
        "overall": overall_json(&snapshot.overall),
        "savings_distribution": savings_distribution_json(&snapshot.savings_distribution),
        "top_roots": snapshot.top_roots.iter().map(command_total_json).collect::<Vec<_>>(),
        "top_commands": snapshot.top_commands.iter().map(command_total_json).collect::<Vec<_>>(),
        "largest_invocations": snapshot
            .largest_invocations
            .iter()
            .map(invocation_json)
            .collect::<Vec<_>>(),
        "recent_invocations": snapshot
            .recent_invocations
            .iter()
            .map(invocation_json)
            .collect::<Vec<_>>(),
        "daily_totals": snapshot.daily_totals.iter().map(daily_json).collect::<Vec<_>>(),
        "presentation": presentation_json(evidence),
        "recommendations": evidence
            .recommendations
            .iter()
            .map(recommendation_json)
            .collect::<Vec<_>>(),
        "command_report_totals": evidence
            .command_report_totals
            .iter()
            .map(command_report_total_json)
            .collect::<Vec<_>>(),
        "command_report_status": super::report_view::status_summary_json(
            &evidence.command_report_status,
        ),
        "command_report_denial_reasons": denial_reason_summary_json(evidence),
        "recent_command_reports": evidence
            .recent_command_reports
            .iter()
            .map(super::report_view::report_json)
            .collect::<Vec<_>>(),
        "recent_failure_artifacts": evidence
            .recent_failure_artifacts
            .iter()
            .map(failure_artifact_json)
            .collect::<Vec<_>>(),
        "passthrough_opportunities": evidence
            .passthrough_opportunities
            .iter()
            .map(opportunity_view::command_opportunity_json)
            .collect::<Vec<_>>(),
        "routing_summary": routing_view::routing_summary_json(&evidence.routing_summary),
        "routing_decision_totals": evidence
            .routing_decision_totals
            .iter()
            .map(routing_view::routing_total_json)
            .collect::<Vec<_>>(),
        "recent_routing_decisions": evidence
            .recent_routing_decisions
            .iter()
            .map(routing_view::routing_decision_json)
            .collect::<Vec<_>>(),
        "operational_health": operational_health_json(evidence),
        "failure_focus": evidence
            .failure_focus
            .iter()
            .map(failure_focus_json)
            .collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map_err(Into::into)
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
    })
}

fn filter_json(filter: &FilterSummary) -> serde_json::Value {
    serde_json::json!({
        "command_root": filter.command_root.as_deref(),
        "command": filter.command.as_deref(),
        "active": !filter.is_empty(),
    })
}

fn command_total_json(total: &CommandTotalInsight) -> serde_json::Value {
    serde_json::json!({
        "command": total.command,
        "invocations": total.invocations,
        "failures": total.failures,
        "expansions": total.expansions,
        "raw": metrics_json(&total.raw),
        "emitted": metrics_json(&total.emitted),
        "saved": metrics_json(&total.saved),
        "expanded": metrics_json(&total.expanded),
        "net_token_delta": signed_delta(total.emitted.tokens, total.raw.tokens),
        "avg_saved_tokens": div_floor(total.saved.tokens, total.invocations),
        "best_saved_chars": total.best_saved_chars,
        "best_saved_tokens": total.best_saved_tokens,
        "best_expanded_tokens": total.best_expanded_tokens,
    })
}

fn invocation_json(invocation: &InvocationInsight) -> serde_json::Value {
    serde_json::json!({
        "id": invocation.id,
        "occurred_at_ms": invocation.occurred_at_ms,
        "process": invocation.process,
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
        "net_token_delta": signed_delta(invocation.expanded.tokens, invocation.saved.tokens),
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

fn argv_json_value(argv_json: &str) -> serde_json::Value {
    serde_json::from_str(argv_json).unwrap_or_else(|_| serde_json::json!([]))
}

fn daily_json(day: &DailyInsight) -> serde_json::Value {
    serde_json::json!({
        "day": day.day,
        "invocations": day.invocations,
        "failures": day.failures,
        "expansions": day.expansions,
        "saved": metrics_json(&day.saved),
        "expanded": metrics_json(&day.expanded),
        "net_token_delta": signed_delta(day.expanded.tokens, day.saved.tokens),
    })
}

fn presentation_json(evidence: &ExportEvidence) -> serde_json::Value {
    let snapshot = &evidence.snapshot;
    let metrics =
        PresentationMetrics::from_snapshot(snapshot, DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS);
    serde_json::json!({
        "context_window_tokens": DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
        "metrics": presentation_metrics_json(&metrics),
        "headlines": lines_json(&format_presentation_headlines(snapshot)),
        "slide_outline": lines_json(&format_presentation_slide_outline(
            snapshot,
            &evidence.recommendations,
        )),
        "speaker_bullets": lines_json(&format_impact_bullets(
            snapshot,
            DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
        )),
        "demo_commands": lines_json(&format_presentation_demo_commands(evidence.limit)),
    })
}

fn presentation_metrics_json(metrics: &PresentationMetrics) -> serde_json::Value {
    serde_json::json!({
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
    })
}

fn recommendation_json(recommendation: &Recommendation) -> serde_json::Value {
    serde_json::json!({
        "title": recommendation.title,
        "command": recommendation.command,
        "evidence": recommendation.evidence,
        "action": recommendation.action,
    })
}

fn command_report_total_json(total: &CommandReportTotalInsight) -> serde_json::Value {
    serde_json::json!({
        "command": total.command,
        "reports": total.reports,
        "open": total.open,
        "resolved": total.resolved,
        "native_parity": total.native_parity,
        "not_reproducible": total.not_reproducible,
        "denied": total.denied,
        "other": total.other,
    })
}

fn denial_reason_summary_json(evidence: &ExportEvidence) -> serde_json::Value {
    let summary = evidence.command_report_denial_reasons;
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

fn operational_health_json(evidence: &ExportEvidence) -> serde_json::Value {
    let coverage = evidence.failure_coverage;
    serde_json::json!({
        "failed_invocations": coverage.failed_invocations,
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
        "artifact_backed_families": coverage.groups_with_retained_artifacts,
        "families_with_retained_artifacts": coverage.groups_with_retained_artifacts,
        "families_with_linked_but_pruned_artifacts": coverage.groups_with_linked_but_pruned_artifacts,
        "routing_rejections": evidence.routing_summary.rejections,
        "routing_passthrough_eligible": evidence.routing_summary.passthrough_eligible,
        "routing_passthrough_disabled": evidence.routing_summary.passthrough_disabled,
        "routing_cx_owned_errors": evidence.routing_summary.cx_owned_errors,
        "quality_reports": evidence.command_report_status.total,
        "open_quality_reports": evidence.command_report_status.open,
        "closed_quality_reports": evidence.command_report_status.closed(),
        "resolved_quality_reports": evidence.command_report_status.resolved,
        "native_parity_reports": evidence.command_report_status.native_parity,
        "not_reproducible_reports": evidence.command_report_status.not_reproducible,
        "denied_quality_reports": evidence.command_report_status.denied,
        "denied_duplicate_reports": evidence.command_report_denial_reasons.duplicate,
        "denied_insufficient_evidence_reports": evidence.command_report_denial_reasons.insufficient_evidence,
        "denied_invalid_reports": evidence.command_report_denial_reasons.invalid,
        "denied_obsolete_reports": evidence.command_report_denial_reasons.obsolete,
        "denied_unsupported_reports": evidence.command_report_denial_reasons.unsupported,
        "denied_low_value_reports": evidence.command_report_denial_reasons.low_value,
        "denied_other_reports": evidence.command_report_denial_reasons.other,
    })
}

fn failure_focus_json(item: &FailureFocus) -> serde_json::Value {
    serde_json::json!({
        "command": item.total.command,
        "failures": item.total.failures,
        "invocations": item.total.invocations,
        "saved": metrics_json(&item.total.saved),
        "artifact_tool": item.artifact_summary.tool_name,
        "artifact_display_dir": item.artifact_summary.display_dir,
        "artifact_count": item.artifact_summary.count,
        "latest_artifact": item.artifact_summary.latest_display_path,
        "detail_rows": item.coverage.detail_rows,
        "linked_detail_rows": item.coverage.linked_detail_rows,
        "linked_invocations": item.coverage.linked_invocations,
        "orphan_detail_rows": item.coverage.orphan_detail_rows,
        "unknown_invocations": item.coverage.unknown_invocations,
        "output_bearing_detail_rows": item.coverage.output_bearing_detail_rows,
        "silent_detail_rows": item.coverage.silent_detail_rows,
        "artifact_linked_detail_rows": item.coverage.artifact_linked_detail_rows,
        "output_gap_detail_rows": item.coverage.output_gap_detail_rows,
        "response_evidence_available": item.coverage.response_evidence_available,
        "artifact_reference_available": item.coverage.artifact_reference_available,
        "has_output_gap": item.has_output_gap(),
        "coverage_unknown": item.coverage_unknown(),
        "linked_but_pruned": item.linked_but_pruned(),
    })
}

fn lines_json(text: &str) -> Vec<String> {
    text.lines().map(ToString::to_string).collect()
}
