use anyhow::Result;

use crate::support::insights::{
    self, CommandReportInsight, CommandReportTotalInsight, CommandTotalInsight, DailyInsight,
    FailureArtifactInsight, InvocationInsight, OverallInsight,
};

use super::csv_view::{push_metric_row, CsvMetricRow};
use super::data::{ExportEvidence, Recommendation};
use super::distribution_view::push_savings_distribution_csv_rows;
use super::failure_coverage::FailureFocus;
use super::format_utils::{div_floor, ratio_value, signed_delta};
use super::opportunity_view;
use super::presentation::{
    format_impact_bullets, format_presentation_demo_commands, format_presentation_headlines,
    format_presentation_slide_outline, PresentationMetrics,
    DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
};
use super::routing_view;

pub(super) const EXPORT_SCHEMA_NAME: &str = "cx-insights-export";
pub(crate) const EXPORT_SCHEMA_VERSION: u64 = 18;

pub(super) fn format_export_json(evidence: &ExportEvidence) -> Result<String> {
    super::export_json::format_export_json(evidence)
}

pub(super) fn metrics_json(metrics: &insights::TextMetrics) -> serde_json::Value {
    serde_json::json!({
        "bytes": metrics.bytes,
        "chars": metrics.chars,
        "lines": metrics.lines,
        "tokens": metrics.tokens,
    })
}

pub(super) fn format_export_csv(evidence: &ExportEvidence) -> String {
    let snapshot = &evidence.snapshot;
    let mut output = String::from(
        "section,rank,metric,value,process,command_family,command,day,invocation_id,source,exit_code,argv_json,command_shape,command_shape_hash\n",
    );
    push_export_metadata_csv_rows(&mut output, evidence);
    push_presentation_metric_csv_rows(
        &mut output,
        &PresentationMetrics::from_snapshot(snapshot, DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS),
    );
    push_savings_distribution_csv_rows(&mut output, &snapshot.savings_distribution);
    if snapshot.no_data() {
        push_metric_row(&mut output, CsvMetricRow::new("overall", "no_data", "true"));
        push_command_report_total_csv_rows(&mut output, &evidence.command_report_totals);
        push_command_report_csv_rows(&mut output, &evidence.recent_command_reports);
        push_failure_artifact_csv_rows(&mut output, &evidence.recent_failure_artifacts);
        opportunity_view::push_opportunity_csv_rows(
            &mut output,
            &evidence.passthrough_opportunities,
        );
        routing_view::push_routing_csv_rows(
            &mut output,
            &evidence.routing_summary,
            &evidence.routing_decision_totals,
            &evidence.recent_routing_decisions,
        );
        return output;
    }

    push_overall_csv_rows(&mut output, &snapshot.overall);
    push_command_total_csv_rows(&mut output, "top_roots", &snapshot.top_roots);
    push_command_total_csv_rows(&mut output, "top_commands", &snapshot.top_commands);
    push_invocation_csv_rows(
        &mut output,
        "largest_invocations",
        &snapshot.largest_invocations,
    );
    push_invocation_csv_rows(
        &mut output,
        "recent_invocations",
        &snapshot.recent_invocations,
    );
    push_daily_csv_rows(&mut output, &snapshot.daily_totals);
    push_presentation_csv_rows(&mut output, evidence);
    push_recommendation_csv_rows(&mut output, &evidence.recommendations);
    push_command_report_total_csv_rows(&mut output, &evidence.command_report_totals);
    push_command_report_csv_rows(&mut output, &evidence.recent_command_reports);
    push_failure_artifact_csv_rows(&mut output, &evidence.recent_failure_artifacts);
    opportunity_view::push_opportunity_csv_rows(&mut output, &evidence.passthrough_opportunities);
    routing_view::push_routing_csv_rows(
        &mut output,
        &evidence.routing_summary,
        &evidence.routing_decision_totals,
        &evidence.recent_routing_decisions,
    );
    push_operational_health_csv_rows(&mut output, evidence);
    push_failure_focus_csv_rows(&mut output, &evidence.failure_focus);
    output
}

fn push_export_metadata_csv_rows(output: &mut String, evidence: &ExportEvidence) {
    push_metric_row(
        output,
        CsvMetricRow::new("metadata", "schema_name", EXPORT_SCHEMA_NAME),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("metadata", "schema_version", EXPORT_SCHEMA_VERSION),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("metadata", "generated_at_ms", evidence.generated_at_ms),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("metadata", "limit", evidence.limit),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("metadata", "database", &evidence.snapshot.database),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("metadata", "filter_active", !evidence.filter.is_empty()),
    );
    if let Some(command_root) = &evidence.filter.command_root {
        push_metric_row(
            output,
            CsvMetricRow::new("metadata", "filter_command_root", command_root),
        );
    }
    if let Some(command) = &evidence.filter.command {
        push_metric_row(
            output,
            CsvMetricRow::new("metadata", "filter_command", command),
        );
    }
}

fn push_overall_csv_rows(output: &mut String, overall: &OverallInsight) {
    push_metric_row(
        output,
        CsvMetricRow::new("overall", "invocations", overall.invocations.to_string()),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("overall", "failures", overall.failures.to_string()),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("overall", "expansions", overall.expansions.to_string()),
    );
    push_metric_group(
        output,
        "overall",
        "raw",
        &overall.raw,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_group(
        output,
        "overall",
        "expanded",
        &overall.expanded,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "overall",
            "net_token_delta",
            signed_delta(overall.emitted.tokens, overall.raw.tokens),
        ),
    );
    push_metric_group(
        output,
        "overall",
        "emitted",
        &overall.emitted,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_group(
        output,
        "overall",
        "saved",
        &overall.saved,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "overall",
            "savings_ratio_percent",
            format!(
                "{:.1}",
                ratio_value(overall.saved.chars, overall.raw.chars) * 100.0
            ),
        ),
    );
}

fn push_command_total_csv_rows(output: &mut String, section: &str, totals: &[CommandTotalInsight]) {
    for (index, total) in totals.iter().enumerate() {
        let mut base = CsvMetricRow::new(section, "", "")
            .rank(index + 1)
            .command(&total.command);
        if section == "top_roots" {
            base = base.process(&total.command);
        } else {
            base = base
                .process(insights::command_root(&total.command))
                .command_family(&total.command);
        }
        push_metric_row(
            output,
            base.clone().metric("invocations", total.invocations),
        );
        push_metric_row(output, base.clone().metric("failures", total.failures));
        push_metric_row(output, base.clone().metric("expansions", total.expansions));
        push_metric_group(output, section, "raw", &total.raw, |_, metric, value| {
            base.clone().metric(metric, value)
        });
        push_metric_group(
            output,
            section,
            "emitted",
            &total.emitted,
            |_, metric, value| base.clone().metric(metric, value),
        );
        push_metric_group(
            output,
            section,
            "expanded",
            &total.expanded,
            |_, metric, value| base.clone().metric(metric, value),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "net_token_delta",
                signed_delta(total.emitted.tokens, total.raw.tokens),
            ),
        );
        push_metric_group(
            output,
            section,
            "saved",
            &total.saved,
            |_, metric, value| base.clone().metric(metric, value),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "avg_saved_tokens",
                div_floor(total.saved.tokens, total.invocations),
            ),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("best_saved_chars", total.best_saved_chars),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("best_saved_tokens", total.best_saved_tokens),
        );
        push_metric_row(
            output,
            base.metric("best_expanded_tokens", total.best_expanded_tokens),
        );
    }
}

fn push_invocation_csv_rows(output: &mut String, section: &str, invocations: &[InvocationInsight]) {
    for (index, invocation) in invocations.iter().enumerate() {
        let base = CsvMetricRow::new(section, "", "")
            .rank(index + 1)
            .process(&invocation.process)
            .command_family(&invocation.command_family)
            .command(&invocation.command)
            .invocation_id(invocation.id)
            .source(&invocation.source)
            .exit_code(invocation.exit_code)
            .argv_json(&invocation.argv_json)
            .command_shape(&invocation.command_shape)
            .command_shape_hash(&invocation.command_shape_hash);
        push_metric_group(
            output,
            section,
            "saved",
            &invocation.saved,
            |_, metric, value| base.clone().metric(metric, value),
        );
        push_metric_group(
            output,
            section,
            "expanded",
            &invocation.expanded,
            |_, metric, value| base.clone().metric(metric, value),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("expansion_reason", &invocation.expansion_reason),
        );
        for (metric, value) in [
            ("binary_version", invocation.binary_version.as_str()),
            ("binary_revision", invocation.binary_revision.as_str()),
            ("binary_fingerprint", invocation.binary_fingerprint.as_str()),
            (
                "emitted_response_preview",
                invocation.emitted_response_preview.as_str(),
            ),
            (
                "raw_response_preview",
                invocation.raw_response_preview.as_str(),
            ),
        ] {
            push_metric_row(output, base.clone().metric(metric, value));
        }
        push_metric_row(
            output,
            base.clone().metric(
                "net_token_delta",
                signed_delta(invocation.expanded.tokens, invocation.saved.tokens),
            ),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "savings_ratio_percent",
                format!("{:.1}", invocation.savings_ratio * 100.0),
            ),
        );
        push_metric_row(
            output,
            base.metric(
                "compression_ratio",
                format!("{:.3}", invocation.compression_ratio),
            ),
        );
    }
}

fn push_failure_artifact_csv_rows(output: &mut String, artifacts: &[FailureArtifactInsight]) {
    for (index, artifact) in artifacts.iter().enumerate() {
        let mut base = CsvMetricRow::new("recent_failure_artifacts", "", "")
            .rank(index + 1)
            .process(&artifact.tool_name)
            .source(&artifact.display_path)
            .exit_code(artifact.exit_code);
        if let Some(invocation_id) = artifact.invocation_id {
            base = base.invocation_id(invocation_id);
        }
        for (metric, value) in [
            ("compression", artifact.compression.clone()),
            ("stdout_bytes", artifact.stdout_bytes.to_string()),
            ("stderr_bytes", artifact.stderr_bytes.to_string()),
            ("original_bytes", artifact.original_bytes.to_string()),
            ("stored_bytes", artifact.stored_bytes.to_string()),
            (
                "report_id",
                artifact
                    .report_id
                    .map_or_else(String::new, |id| id.to_string()),
            ),
            ("binary_revision", artifact.binary_revision.clone()),
            ("binary_fingerprint", artifact.binary_fingerprint.clone()),
        ] {
            push_metric_row(output, base.clone().metric(metric, value));
        }
    }
}

fn push_daily_csv_rows(output: &mut String, days: &[DailyInsight]) {
    for (index, day) in days.iter().enumerate() {
        let base = CsvMetricRow::new("daily_totals", "", "")
            .rank(index + 1)
            .day(&day.day);
        push_metric_row(output, base.clone().metric("invocations", day.invocations));
        push_metric_row(output, base.clone().metric("failures", day.failures));
        push_metric_row(output, base.clone().metric("expansions", day.expansions));
        push_metric_group(
            output,
            "daily_totals",
            "saved",
            &day.saved,
            |_, metric, value| base.clone().metric(metric, value),
        );
        push_metric_group(
            output,
            "daily_totals",
            "expanded",
            &day.expanded,
            |_, metric, value| base.clone().metric(metric, value),
        );
        push_metric_row(
            output,
            base.metric(
                "net_token_delta",
                signed_delta(day.expanded.tokens, day.saved.tokens),
            ),
        );
    }
}

fn push_presentation_csv_rows(output: &mut String, evidence: &ExportEvidence) {
    push_text_lines_as_rows(
        output,
        "presentation",
        "headline",
        &format_presentation_headlines(&evidence.snapshot),
    );
    push_text_lines_as_rows(
        output,
        "presentation",
        "slide",
        &format_presentation_slide_outline(&evidence.snapshot, &evidence.recommendations),
    );
    push_text_lines_as_rows(
        output,
        "presentation",
        "speaker_bullet",
        &format_impact_bullets(
            &evidence.snapshot,
            DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
        ),
    );
    push_text_lines_as_rows(
        output,
        "presentation",
        "demo_command",
        &format_presentation_demo_commands(evidence.limit),
    );
}

fn push_presentation_metric_csv_rows(output: &mut String, metrics: &PresentationMetrics) {
    push_metric_row(
        output,
        CsvMetricRow::new("presentation_metrics", "invocations", metrics.invocations),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("presentation_metrics", "failures", metrics.failures),
    );
    push_metric_row(
        output,
        CsvMetricRow::new("presentation_metrics", "expansions", metrics.expansions),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "presentation_metrics",
            "failure_rate",
            format!("{:.6}", metrics.failure_rate),
        ),
    );
    push_metric_group(
        output,
        "presentation_metrics",
        "raw",
        &metrics.raw,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_group(
        output,
        "presentation_metrics",
        "expanded",
        &metrics.expanded,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "presentation_metrics",
            "net_token_delta",
            metrics.net_token_delta,
        ),
    );
    push_metric_group(
        output,
        "presentation_metrics",
        "emitted",
        &metrics.emitted,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_group(
        output,
        "presentation_metrics",
        "saved",
        &metrics.saved,
        |section, metric, value| CsvMetricRow::new(section, metric, value),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "presentation_metrics",
            "savings_ratio",
            format!("{:.6}", metrics.savings_ratio),
        ),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "presentation_metrics",
            "average_saved_tokens",
            metrics.average_saved_tokens,
        ),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "presentation_metrics",
            "context_window_tokens",
            metrics.context_window_tokens,
        ),
    );
    push_metric_row(
        output,
        CsvMetricRow::new(
            "presentation_metrics",
            "context_windows_saved",
            format!("{:.6}", metrics.context_windows_saved),
        ),
    );
}

fn push_recommendation_csv_rows(output: &mut String, recommendations: &[Recommendation]) {
    for (index, recommendation) in recommendations.iter().enumerate() {
        let base = CsvMetricRow::new("recommendations", "", "")
            .rank(index + 1)
            .command(&recommendation.command);
        push_metric_row(output, base.clone().metric("title", &recommendation.title));
        push_metric_row(
            output,
            base.clone().metric("evidence", &recommendation.evidence),
        );
        push_metric_row(output, base.metric("action", &recommendation.action));
    }
}

fn push_command_report_total_csv_rows(output: &mut String, totals: &[CommandReportTotalInsight]) {
    for (index, total) in totals.iter().enumerate() {
        let base = CsvMetricRow::new("command_report_totals", "", "")
            .rank(index + 1)
            .command_root(insights::command_root(&total.command))
            .command(&total.command);
        push_metric_row(output, base.clone().metric("reports", total.reports));
        push_metric_row(output, base.clone().metric("open", total.open));
        push_metric_row(output, base.clone().metric("resolved", total.resolved));
        push_metric_row(
            output,
            base.clone().metric("native_parity", total.native_parity),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("not_reproducible", total.not_reproducible),
        );
        push_metric_row(output, base.clone().metric("denied", total.denied));
        push_metric_row(output, base.metric("other", total.other));
    }
}

fn push_command_report_csv_rows(output: &mut String, reports: &[CommandReportInsight]) {
    for (index, report) in reports.iter().enumerate() {
        let base = CsvMetricRow::new("recent_command_reports", "", "")
            .rank(index + 1)
            .process(&report.command_root)
            .command_family(&report.command_family)
            .command(&report.command_family)
            .invocation_id(report.id);
        push_metric_row(
            output,
            base.clone().metric("issue_kind", &report.issue_kind),
        );
        push_metric_row(
            output,
            base.clone().metric("reported_command", &report.command),
        );
        push_metric_row(output, base.clone().metric("note", &report.note));
        push_metric_row(output, base.clone().metric("status", &report.status));
        push_metric_row(
            output,
            base.clone().metric("evidence_kind", &report.evidence_kind),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "evidence_invocation_id",
                report.invocation_id.unwrap_or_default(),
            ),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "cx_response_recorded",
                u64::from(!report.cx_response.is_empty()),
            ),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "native_response_recorded",
                u64::from(!report.native_response.is_empty()),
            ),
        );
        push_metric_row(
            output,
            base.clone().metric("artifact_ref", &report.artifact_ref),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("resolution_note", &report.resolution_note),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("resolution_revision", &report.resolution_revision),
        );
        push_metric_row(
            output,
            base.clone().metric("denial_reason", &report.denial_reason),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "related_report_id",
                report.related_report_id.unwrap_or_default(),
            ),
        );
        push_metric_row(
            output,
            base.metric("status_updated_at_ms", report.status_updated_at_ms),
        );
    }
}

fn push_operational_health_csv_rows(output: &mut String, evidence: &ExportEvidence) {
    push_failure_health_csv_rows(output, evidence);
    push_routing_and_report_health_csv_rows(output, evidence);
}

fn push_failure_health_csv_rows(output: &mut String, evidence: &ExportEvidence) {
    let coverage = evidence.failure_coverage;
    push_metric_row(
        output,
        CsvMetricRow::new(
            "operational_health",
            "failed_invocations",
            coverage.failed_invocations,
        ),
    );
    for (metric, value) in [
        ("failing_command_families", coverage.failing_groups),
        ("detailed_failure_rows", coverage.detail_rows),
        ("linked_failure_detail_rows", coverage.linked_detail_rows),
        ("orphan_failure_detail_rows", coverage.orphan_detail_rows),
        ("unknown_failure_invocations", coverage.unknown_invocations),
        (
            "output_bearing_failure_rows",
            coverage.output_bearing_detail_rows,
        ),
        ("silent_failure_rows", coverage.silent_detail_rows),
        (
            "artifact_linked_failure_rows",
            coverage.artifact_linked_detail_rows,
        ),
        (
            "output_without_artifact_rows",
            coverage.output_gap_detail_rows,
        ),
        ("missing_artifact_risks", coverage.groups_with_output_gaps),
        (
            "families_with_output_gaps",
            coverage.groups_with_output_gaps,
        ),
        (
            "families_with_unknown_coverage",
            coverage.groups_with_unknown_coverage,
        ),
        (
            "artifact_backed_families",
            coverage.groups_with_retained_artifacts,
        ),
        (
            "families_with_retained_artifacts",
            coverage.groups_with_retained_artifacts,
        ),
        (
            "families_with_linked_but_pruned_artifacts",
            coverage.groups_with_linked_but_pruned_artifacts,
        ),
    ] {
        push_metric_row(
            output,
            CsvMetricRow::new("operational_health", metric, value),
        );
    }
}

fn push_routing_and_report_health_csv_rows(output: &mut String, evidence: &ExportEvidence) {
    for (metric, value) in [
        ("routing_rejections", evidence.routing_summary.rejections),
        (
            "routing_passthrough_eligible",
            evidence.routing_summary.passthrough_eligible,
        ),
        (
            "routing_passthrough_disabled",
            evidence.routing_summary.passthrough_disabled,
        ),
        (
            "routing_cx_owned_errors",
            evidence.routing_summary.cx_owned_errors,
        ),
        ("quality_reports", evidence.command_report_status.total),
        ("open_quality_reports", evidence.command_report_status.open),
        (
            "closed_quality_reports",
            evidence.command_report_status.closed(),
        ),
        (
            "native_parity_reports",
            evidence.command_report_status.native_parity,
        ),
        (
            "not_reproducible_reports",
            evidence.command_report_status.not_reproducible,
        ),
        (
            "denied_quality_reports",
            evidence.command_report_status.denied,
        ),
        (
            "denied_duplicate_reports",
            evidence.command_report_denial_reasons.duplicate,
        ),
        (
            "denied_insufficient_evidence_reports",
            evidence.command_report_denial_reasons.insufficient_evidence,
        ),
        (
            "denied_invalid_reports",
            evidence.command_report_denial_reasons.invalid,
        ),
        (
            "denied_obsolete_reports",
            evidence.command_report_denial_reasons.obsolete,
        ),
        (
            "denied_unsupported_reports",
            evidence.command_report_denial_reasons.unsupported,
        ),
        (
            "denied_low_value_reports",
            evidence.command_report_denial_reasons.low_value,
        ),
        (
            "denied_other_reports",
            evidence.command_report_denial_reasons.other,
        ),
    ] {
        push_metric_row(
            output,
            CsvMetricRow::new("operational_health", metric, value),
        );
    }
}

fn push_failure_focus_csv_rows(output: &mut String, focus: &[FailureFocus]) {
    for (index, item) in focus.iter().enumerate() {
        let base = CsvMetricRow::new("failure_focus", "", "")
            .rank(index + 1)
            .command_root(insights::command_root(&item.total.command))
            .command(&item.total.command);
        push_metric_row(output, base.clone().metric("failures", item.total.failures));
        push_metric_row(
            output,
            base.clone().metric("invocations", item.total.invocations),
        );
        push_metric_row(
            output,
            base.clone().metric("saved_tokens", item.total.saved.tokens),
        );
        for (metric, value) in [
            ("detail_rows", item.coverage.detail_rows),
            ("linked_detail_rows", item.coverage.linked_detail_rows),
            ("orphan_detail_rows", item.coverage.orphan_detail_rows),
            ("unknown_invocations", item.coverage.unknown_invocations),
            (
                "output_bearing_detail_rows",
                item.coverage.output_bearing_detail_rows,
            ),
            ("silent_detail_rows", item.coverage.silent_detail_rows),
            (
                "artifact_linked_detail_rows",
                item.coverage.artifact_linked_detail_rows,
            ),
            (
                "output_gap_detail_rows",
                item.coverage.output_gap_detail_rows,
            ),
        ] {
            push_metric_row(output, base.clone().metric(metric, value));
        }
        push_metric_row(
            output,
            base.clone()
                .metric("artifact_tool", &item.artifact_summary.tool_name),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("artifact_count", item.artifact_summary.count),
        );
        push_metric_row(
            output,
            base.clone().metric(
                "latest_artifact",
                item.artifact_summary
                    .latest_display_path
                    .as_deref()
                    .unwrap_or("-"),
            ),
        );
        push_metric_row(
            output,
            base.clone().metric("has_output_gap", item.has_output_gap()),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("coverage_unknown", item.coverage_unknown()),
        );
        push_metric_row(
            output,
            base.metric("linked_but_pruned", item.linked_but_pruned()),
        );
    }
}

fn push_text_lines_as_rows(output: &mut String, section: &str, metric: &str, text: &str) {
    for (index, line) in text.lines().enumerate() {
        push_metric_row(
            output,
            CsvMetricRow::new(section, metric, line).rank(index + 1),
        );
    }
}

pub(super) fn push_metric_group<F>(
    output: &mut String,
    section: &str,
    prefix: &str,
    metrics: &insights::TextMetrics,
    mut row_builder: F,
) where
    F: FnMut(&str, &str, u64) -> CsvMetricRow,
{
    push_metric_row(
        output,
        row_builder(section, &format!("{prefix}_bytes"), metrics.bytes),
    );
    push_metric_row(
        output,
        row_builder(section, &format!("{prefix}_chars"), metrics.chars),
    );
    push_metric_row(
        output,
        row_builder(section, &format!("{prefix}_lines"), metrics.lines),
    );
    push_metric_row(
        output,
        row_builder(section, &format!("{prefix}_tokens"), metrics.tokens),
    );
}
