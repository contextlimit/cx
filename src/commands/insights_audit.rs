use std::collections::BTreeMap;

use anyhow::Result;

use super::insights::failure_coverage::{
    load_failure_coverage, FailureCoverageSummary, FailureFocus,
};
use crate::support::{
    insights::{
        self, CommandFilter, CommandLevel, CommandReportDenialReasonSummary,
        CommandReportStatusSummary, CommandReportTotalInsight, CommandTotalInsight,
        CommandTotalSort, InvocationInsight, OverallInsight, TextMetrics,
    },
    runner::ProxyOutcome,
};

const AUDIT_SCHEMA_NAME: &str = "cx-insights-audit";
const AUDIT_SCHEMA_VERSION: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditFormat {
    Text,
    Json,
}

pub fn run(limit: usize, filter: CommandFilter<'_>, format: AuditFormat) -> Result<ProxyOutcome> {
    let evidence = AuditEvidence::load(limit.clamp(1, 100), filter)?;
    let output = match format {
        AuditFormat::Text => format_audit(&evidence),
        AuditFormat::Json => serde_json::to_string_pretty(&audit_json(&evidence))?,
    };
    Ok(ProxyOutcome::success(output))
}

struct AuditEvidence {
    database: String,
    limit: usize,
    filter: FilterSummary,
    overall: OverallInsight,
    roots_by_saved: Vec<CommandTotalInsight>,
    roots_by_failures: Vec<CommandTotalInsight>,
    report_roots: Vec<CommandReportTotalInsight>,
    report_status: CommandReportStatusSummary,
    report_denial_reasons: CommandReportDenialReasonSummary,
    failure_focus: Vec<FailureFocus>,
    failure_coverage: FailureCoverageSummary,
    recent_insights_invocations: Vec<InvocationInsight>,
}

impl AuditEvidence {
    fn load(limit: usize, filter: CommandFilter<'_>) -> Result<Self> {
        let roots_by_failures = insights::command_totals_at_level_filtered(
            CommandLevel::Root,
            CommandTotalSort::Failures,
            limit,
            filter,
        )?;
        let failure_coverage = load_failure_coverage(CommandLevel::Root, filter)?;
        Ok(Self {
            database: insights::insights_database_path()?.display().to_string(),
            limit,
            filter: FilterSummary::from_filter(filter),
            overall: insights::overall_totals_filtered(filter)?,
            roots_by_saved: insights::command_totals_at_level_filtered(
                CommandLevel::Root,
                CommandTotalSort::Tokens,
                limit,
                filter,
            )?,
            roots_by_failures: roots_by_failures.clone(),
            report_roots: insights::command_report_totals_at_level(
                CommandLevel::Root,
                limit,
                filter,
            )?,
            report_status: insights::command_report_status_summary(filter)?,
            report_denial_reasons: insights::command_report_denial_reason_summary(filter)?,
            failure_focus: failure_coverage.rows.into_iter().take(limit).collect(),
            failure_coverage: failure_coverage.summary,
            recent_insights_invocations: insights::recent_invocations_filtered(
                20,
                CommandFilter {
                    command_root: Some("insights"),
                    command: None,
                },
            )?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct FilterSummary {
    command_root: Option<String>,
    command: Option<String>,
}

impl FilterSummary {
    fn from_filter(filter: CommandFilter<'_>) -> Self {
        Self {
            command_root: filter.command_root.map(ToString::to_string),
            command: filter.command.map(ToString::to_string),
        }
    }

    fn is_empty(&self) -> bool {
        self.command_root.is_none() && self.command.is_none()
    }
}

fn format_audit(evidence: &AuditEvidence) -> String {
    let mut output = String::from("cx insights: drift audit\n");
    output.push_str(&format!(
        "Database: {}\nLimit: {}\n",
        evidence.database, evidence.limit
    ));
    output.push_str(&format_filter_line(&evidence.filter));
    output.push('\n');
    output.push_str(&format_summary(evidence));
    output.push_str("\n\nDrift signals:\n");
    output.push_str(&format_drift_signals(evidence));
    output.push_str("\n\nSavings concentration by command root:\n");
    output.push_str(&format_root_savings(evidence));
    output.push_str("\n\nFailure and artifact coverage:\n");
    output.push_str(&format_failure_focus(evidence));
    output.push_str("\n\nCommand-quality reports:\n");
    output.push_str(&format_report_roots(evidence));
    output.push_str("\n\nFollow-up commands:\n");
    output.push_str(&format_follow_up_commands(evidence));
    output
}

fn format_summary(evidence: &AuditEvidence) -> String {
    format!(
        "Summary: {} invocations; {} saved estimated tokens; {} saved lines; {} failures ({}); {} open quality reports, {} denied ({} total); {} output-bearing artifact gaps across {} roots; {} failures with unknown detail coverage.",
        format_count(evidence.overall.invocations),
        format_count(evidence.overall.saved.tokens),
        format_count(evidence.overall.saved.lines),
        format_count(evidence.overall.failures),
        format_ratio(evidence.overall.failures, evidence.overall.invocations),
        format_count(evidence.report_status.open),
        format_count(evidence.report_status.denied),
        format_count(evidence.report_status.total),
        format_count(evidence.failure_coverage.output_gap_detail_rows),
        format_count(evidence.failure_coverage.groups_with_output_gaps),
        format_count(evidence.failure_coverage.unknown_invocations),
    )
}

fn format_drift_signals(evidence: &AuditEvidence) -> String {
    let mut lines = Vec::new();
    push_savings_concentration_signal(evidence, &mut lines);
    push_failure_hotspot_signal(evidence, &mut lines);
    push_quality_report_signal(evidence, &mut lines);
    push_artifact_signal(evidence, &mut lines);
    push_self_recording_signal(evidence, &mut lines);
    if lines.is_empty() {
        lines.push("- No drift signals found for the current filter.".to_string());
    }
    lines.join("\n")
}

fn audit_json(evidence: &AuditEvidence) -> serde_json::Value {
    serde_json::json!({
        "schema_name": AUDIT_SCHEMA_NAME,
        "schema_version": AUDIT_SCHEMA_VERSION,
        "database": evidence.database,
        "limit": evidence.limit,
        "filter": filter_json(&evidence.filter),
        "summary": summary_json(evidence),
        "drift_signals": drift_signal_json(evidence),
        "savings_concentration": evidence
            .roots_by_saved
            .iter()
            .map(|total| root_savings_json(evidence, total))
            .collect::<Vec<_>>(),
        "failure_artifact_coverage": evidence
            .failure_focus
            .iter()
            .map(failure_focus_json)
            .collect::<Vec<_>>(),
        "quality_reports": evidence
            .report_roots
            .iter()
            .map(|total| report_root_json(evidence, total))
            .collect::<Vec<_>>(),
        "insights_self_recording": {
            "recent_invocations": evidence.recent_insights_invocations.len(),
            "clear": evidence.recent_insights_invocations.is_empty(),
        },
        "follow_up_commands": follow_up_commands(evidence),
    })
}

fn summary_json(evidence: &AuditEvidence) -> serde_json::Value {
    serde_json::json!({
        "invocations": evidence.overall.invocations,
        "failures": evidence.overall.failures,
        "failure_rate_percent": ratio_percent(evidence.overall.failures, evidence.overall.invocations),
        "quality_reports": total_reports(evidence),
        "quality_report_status": super::insights::report_view::status_summary_json(&evidence.report_status),
        "quality_report_denial_reasons": denial_reason_summary_json(evidence.report_denial_reasons),
        "missing_artifact_risks": evidence.failure_coverage.groups_with_output_gaps,
        "artifact_coverage": failure_coverage_summary_json(evidence.failure_coverage),
        "saved": metrics_json(&evidence.overall.saved),
        "raw": metrics_json(&evidence.overall.raw),
        "emitted": metrics_json(&evidence.overall.emitted),
    })
}

fn denial_reason_summary_json(summary: CommandReportDenialReasonSummary) -> serde_json::Value {
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

fn drift_signal_json(evidence: &AuditEvidence) -> Vec<serde_json::Value> {
    let mut signals = Vec::new();
    if let Some(top) = evidence.roots_by_saved.first() {
        signals.push(serde_json::json!({
            "kind": "savings_concentration",
            "command": top.command,
            "saved_tokens": top.saved.tokens,
            "share_percent": ratio_percent(top.saved.tokens, evidence.overall.saved.tokens),
        }));
    }
    if let Some(top) = top_failure_rate_root(evidence) {
        signals.push(serde_json::json!({
            "kind": "failure_hotspot",
            "command": top.command,
            "failures": top.failures,
            "invocations": top.invocations,
            "failure_rate_percent": ratio_percent(top.failures, top.invocations),
        }));
    }
    if let Some(top) = top_open_report_root(evidence) {
        signals.push(serde_json::json!({
            "kind": "quality_report_concentration",
            "command": top.command,
            "open_reports": top.open,
            "share_percent": ratio_percent(top.open, evidence.report_status.open),
        }));
    }
    signals.push(serde_json::json!({
        "kind": "artifact_coverage",
        "output_gap_detail_rows": evidence.failure_coverage.output_gap_detail_rows,
        "roots_with_output_gaps": evidence.failure_coverage.groups_with_output_gaps,
        "unknown_invocations": evidence.failure_coverage.unknown_invocations,
        "clear": evidence.failure_coverage.groups_with_output_gaps == 0,
    }));
    signals.push(serde_json::json!({
        "kind": "insights_self_recording",
        "recent_invocations": evidence.recent_insights_invocations.len(),
        "clear": evidence.recent_insights_invocations.is_empty(),
    }));
    signals
}

fn filter_json(filter: &FilterSummary) -> serde_json::Value {
    serde_json::json!({
        "active": !filter.is_empty(),
        "command_root": filter.command_root.as_deref(),
        "command": filter.command.as_deref(),
    })
}

fn root_savings_json(evidence: &AuditEvidence, total: &CommandTotalInsight) -> serde_json::Value {
    let reports = report_counts_by_root(evidence)
        .get(total.command.as_str())
        .copied()
        .unwrap_or(0);
    serde_json::json!({
        "command": total.command,
        "invocations": total.invocations,
        "failures": total.failures,
        "failure_rate_percent": ratio_percent(total.failures, total.invocations),
        "saved": metrics_json(&total.saved),
        "saved_share_percent": ratio_percent(total.saved.tokens, evidence.overall.saved.tokens),
        "open_quality_reports": reports,
    })
}

fn failure_coverage_summary_json(summary: FailureCoverageSummary) -> serde_json::Value {
    serde_json::json!({
        "failing_roots": summary.failing_groups,
        "failed_invocations": summary.failed_invocations,
        "detail_rows": summary.detail_rows,
        "linked_detail_rows": summary.linked_detail_rows,
        "orphan_detail_rows": summary.orphan_detail_rows,
        "unknown_invocations": summary.unknown_invocations,
        "output_bearing_detail_rows": summary.output_bearing_detail_rows,
        "silent_detail_rows": summary.silent_detail_rows,
        "artifact_linked_detail_rows": summary.artifact_linked_detail_rows,
        "output_gap_detail_rows": summary.output_gap_detail_rows,
        "roots_with_output_gaps": summary.groups_with_output_gaps,
        "roots_with_unknown_coverage": summary.groups_with_unknown_coverage,
        "roots_with_linked_but_pruned_artifacts": summary.groups_with_linked_but_pruned_artifacts,
        "roots_with_retained_artifacts": summary.groups_with_retained_artifacts,
    })
}

fn failure_focus_json(row: &FailureFocus) -> serde_json::Value {
    serde_json::json!({
        "command": row.total.command,
        "failures": row.coverage.failed_invocations,
        "invocations": row.total.invocations,
        "failure_rate_percent": ratio_percent(row.coverage.failed_invocations, row.total.invocations),
        "saved_tokens": row.total.saved.tokens,
        "detail_rows": row.coverage.detail_rows,
        "linked_detail_rows": row.coverage.linked_detail_rows,
        "linked_invocations": row.coverage.linked_invocations,
        "orphan_detail_rows": row.coverage.orphan_detail_rows,
        "unknown_invocations": row.coverage.unknown_invocations,
        "output_bearing_detail_rows": row.coverage.output_bearing_detail_rows,
        "silent_detail_rows": row.coverage.silent_detail_rows,
        "artifact_linked_detail_rows": row.coverage.artifact_linked_detail_rows,
        "output_gap_detail_rows": row.coverage.output_gap_detail_rows,
        "artifact_tool": row.artifact_summary.tool_name,
        "artifact_count": row.artifact_summary.count,
        "latest_artifact": row.artifact_summary.latest_display_path,
        "has_output_gap": row.has_output_gap(),
        "coverage_unknown": row.coverage_unknown(),
        "linked_but_pruned": row.linked_but_pruned(),
    })
}

fn report_root_json(
    evidence: &AuditEvidence,
    total: &CommandReportTotalInsight,
) -> serde_json::Value {
    serde_json::json!({
        "command": total.command,
        "reports": total.reports,
        "open": total.open,
        "resolved": total.resolved,
        "native_parity": total.native_parity,
        "not_reproducible": total.not_reproducible,
        "denied": total.denied,
        "other": total.other,
        "share_percent": ratio_percent(total.reports, total_reports(evidence)),
    })
}

fn metrics_json(metrics: &TextMetrics) -> serde_json::Value {
    serde_json::json!({
        "bytes": metrics.bytes,
        "chars": metrics.chars,
        "lines": metrics.lines,
        "tokens": metrics.tokens,
    })
}

fn push_savings_concentration_signal(evidence: &AuditEvidence, lines: &mut Vec<String>) {
    if let Some(top) = evidence.roots_by_saved.first() {
        lines.push(format!(
            "- `{}` owns {} of saved tokens ({} saved tokens across {} invocations).",
            top.command,
            format_ratio(top.saved.tokens, evidence.overall.saved.tokens),
            format_count(top.saved.tokens),
            format_count(top.invocations),
        ));
    }
}

fn push_failure_hotspot_signal(evidence: &AuditEvidence, lines: &mut Vec<String>) {
    if let Some(top) = top_failure_rate_root(evidence) {
        lines.push(format!(
            "- `{}` has the highest failure rate among failing roots: {} failures / {} invocations ({}).",
            top.command,
            format_count(top.failures),
            format_count(top.invocations),
            format_ratio(top.failures, top.invocations),
        ));
    }
}

fn push_quality_report_signal(evidence: &AuditEvidence, lines: &mut Vec<String>) {
    if let Some(top) = top_open_report_root(evidence) {
        lines.push(format!(
            "- `{}` has the most open command-quality reports: {} of {} open reports.",
            top.command,
            format_count(top.open),
            format_count(evidence.report_status.open),
        ));
    }
}

fn push_artifact_signal(evidence: &AuditEvidence, lines: &mut Vec<String>) {
    let coverage = evidence.failure_coverage;
    if coverage.groups_with_output_gaps == 0 {
        lines.push(
            "- No recorded output-bearing failure details lack artifact references.".to_string(),
        );
    } else {
        lines.push(format!(
            "- {} output-bearing failure details across {} roots lack artifact references.",
            format_count(coverage.output_gap_detail_rows),
            format_count(coverage.groups_with_output_gaps),
        ));
    }
    if coverage.unknown_invocations > 0 {
        lines.push(format!(
            "- {} failed invocations have no linked failure-detail evidence.",
            format_count(coverage.unknown_invocations),
        ));
    }
    if coverage.groups_with_linked_but_pruned_artifacts > 0 {
        lines.push(format!(
            "- {} roots have artifact-linked history but no currently retained artifact file.",
            format_count(coverage.groups_with_linked_but_pruned_artifacts),
        ));
    }
}

fn push_self_recording_signal(evidence: &AuditEvidence, lines: &mut Vec<String>) {
    if evidence.recent_insights_invocations.is_empty() {
        lines.push("- No recent `insights` self-recording invocations found.".to_string());
    } else {
        lines.push(format!(
            "- Found {} recent `insights` invocations; inspect self-recording guard.",
            format_count(evidence.recent_insights_invocations.len() as u64),
        ));
    }
}

fn format_root_savings(evidence: &AuditEvidence) -> String {
    if evidence.roots_by_saved.is_empty() {
        return "(no command-root savings)\n".to_string();
    }
    let report_counts = report_counts_by_root(evidence);
    let mut output = String::from(
        "root | invocations | failures | failure rate | saved tokens | saved share | open quality reports\n",
    );
    for total in &evidence.roots_by_saved {
        let reports = report_counts
            .get(total.command.as_str())
            .copied()
            .unwrap_or(0);
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {}\n",
            total.command,
            format_count(total.invocations),
            format_count(total.failures),
            format_ratio(total.failures, total.invocations),
            format_count(total.saved.tokens),
            format_ratio(total.saved.tokens, evidence.overall.saved.tokens),
            format_count(reports),
        ));
    }
    output.trim_end().to_string()
}

fn format_failure_focus(evidence: &AuditEvidence) -> String {
    if evidence.failure_focus.is_empty() {
        return "(no failing command roots)\n".to_string();
    }
    let mut output = String::from(
        "root | failures | details | unknown | output gaps | linked | retained | latest retained artifact\n",
    );
    for row in &evidence.failure_focus {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {}\n",
            row.total.command,
            format_count(row.coverage.failed_invocations),
            format_count(row.coverage.detail_rows),
            format_count(row.coverage.unknown_invocations),
            format_count(row.coverage.output_gap_detail_rows),
            format_count(row.coverage.artifact_linked_detail_rows),
            format_count(row.artifact_summary.count as u64),
            row.artifact_summary
                .latest_display_path
                .as_deref()
                .unwrap_or("-"),
        ));
    }
    output.trim_end().to_string()
}

fn format_report_roots(evidence: &AuditEvidence) -> String {
    if evidence.report_roots.is_empty() {
        return "(no command-quality reports)\n".to_string();
    }
    let mut output = String::from(
        "root | total | open | resolved | native parity | not reproducible | denied | other | total share\n",
    );
    let total_reports = total_reports(evidence);
    for total in &evidence.report_roots {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            total.command,
            format_count(total.reports),
            format_count(total.open),
            format_count(total.resolved),
            format_count(total.native_parity),
            format_count(total.not_reproducible),
            format_count(total.denied),
            format_count(total.other),
            format_ratio(total.reports, total_reports),
        ));
    }
    output.trim_end().to_string()
}

fn format_follow_up_commands(evidence: &AuditEvidence) -> String {
    follow_up_commands(evidence).join("\n")
}

fn follow_up_commands(evidence: &AuditEvidence) -> Vec<String> {
    let mut commands = vec![
        format!("cx insights dashboard --limit {}", evidence.limit),
        format!(
            "cx insights failures --level root --limit {}",
            evidence.limit
        ),
        format!(
            "cx insights reports --status open --level root --limit {}",
            evidence.limit
        ),
    ];
    if let Some(top) = evidence.roots_by_saved.first() {
        commands.push(format!(
            "cx insights largest --root {} --limit {}",
            shell_word(&top.command),
            evidence.limit,
        ));
    }
    commands
}

fn format_filter_line(filter: &FilterSummary) -> String {
    if filter.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(root) = &filter.command_root {
        parts.push(format!("root={root}"));
    }
    if let Some(command) = &filter.command {
        parts.push(format!("command={command}"));
    }
    format!("Filter: {}\n", parts.join(", "))
}

fn report_counts_by_root(evidence: &AuditEvidence) -> BTreeMap<&str, u64> {
    evidence
        .report_roots
        .iter()
        .map(|total| (total.command.as_str(), total.open))
        .collect()
}

fn total_reports(evidence: &AuditEvidence) -> u64 {
    evidence.report_status.total
}

fn top_open_report_root(evidence: &AuditEvidence) -> Option<&CommandReportTotalInsight> {
    evidence
        .report_roots
        .iter()
        .filter(|total| total.open > 0)
        .max_by_key(|total| total.open)
}

fn top_failure_rate_root(evidence: &AuditEvidence) -> Option<&CommandTotalInsight> {
    evidence
        .roots_by_failures
        .iter()
        .filter(|total| total.failures > 0)
        .max_by_key(|total| (failure_rate_basis_points(total), total.failures))
}

fn failure_rate_basis_points(total: &CommandTotalInsight) -> u64 {
    total
        .failures
        .saturating_mul(10_000)
        .checked_div(total.invocations)
        .unwrap_or(0)
}

fn format_ratio(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0.0%".to_string();
    }
    format!("{:.1}%", (part as f64 / whole as f64) * 100.0)
}

fn ratio_percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) * 100.0
    }
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    output.chars().rev().collect()
}

fn shell_word(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_alphanumeric() || "_-./".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::support::insights::{
        record_command_report, record_invocation, record_invocation_with_context,
        record_invocation_with_context_and_failure, CommandReportRecord, FailureDetailRecord,
        InvocationContext, InvocationRecord, OutputObservation, TextMetrics,
    };

    #[test]
    fn run_audit_renders_drift_signals_and_tables() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".cx/cache/failures/grep")).unwrap();
        fs::write(home.join(".cx/cache/failures/grep/001.log"), "grep failure").unwrap();
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
                seed_audit_fixture();
                let output = run(5, CommandFilter::default(), AuditFormat::Text).unwrap();

                assert!(output.stdout.contains("cx insights: drift audit"));
                assert!(output.stdout.contains("Summary: 2 invocations"));
                assert!(output.stdout.contains("Drift signals:"));
                assert!(output.stdout.contains("`grep` owns"));
                assert!(output.stdout.contains(
                    "No recorded output-bearing failure details lack artifact references"
                ));
                assert!(output
                    .stdout
                    .contains("1 failed invocations have no linked failure-detail evidence"));
                assert!(output
                    .stdout
                    .contains("No recent `insights` self-recording invocations found"));
                assert!(output.stdout.contains("grep | 1 | 1 | 100.0%"));
                assert!(output
                    .stdout
                    .contains("grep | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 100.0%"));
                assert!(output
                    .stdout
                    .contains("cx insights largest --root grep --limit 5"));
            },
        );
    }

    #[test]
    fn run_audit_can_render_json_for_tools() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".cx/cache/failures/grep")).unwrap();
        fs::write(home.join(".cx/cache/failures/grep/001.log"), "grep failure").unwrap();
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
                seed_audit_fixture();
                let output = run(5, CommandFilter::default(), AuditFormat::Json).unwrap();
                let parsed: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();

                assert_eq!(parsed["schema_name"], "cx-insights-audit");
                assert_eq!(parsed["schema_version"], 4);
                assert_eq!(parsed["summary"]["invocations"], 2);
                assert_eq!(parsed["summary"]["quality_reports"], 1);
                assert_eq!(parsed["summary"]["quality_report_status"]["denied"], 0);
                assert_eq!(parsed["summary"]["quality_report_status"]["open"], 1);
                assert_eq!(
                    parsed["summary"]["artifact_coverage"]["unknown_invocations"],
                    1
                );
                assert_eq!(
                    parsed["summary"]["artifact_coverage"]["output_gap_detail_rows"],
                    0
                );
                assert_eq!(parsed["savings_concentration"][0]["command"], "grep");
                assert_eq!(parsed["quality_reports"][0]["command"], "grep");
                assert!(parsed["drift_signals"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|signal| signal["kind"] == "savings_concentration"));
                assert!(parsed["drift_signals"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|signal| signal["kind"] == "insights_self_recording"));
                assert!(parsed["follow_up_commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|command| command == "cx insights dashboard --limit 5"));
            },
        );
    }

    #[test]
    fn run_audit_supports_root_filter() {
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
                seed_audit_fixture();
                let output = run(
                    5,
                    CommandFilter {
                        command_root: Some("read"),
                        command: None,
                    },
                    AuditFormat::Text,
                )
                .unwrap();

                assert!(output.stdout.contains("Filter: root=read"));
                assert!(output.stdout.contains("read | 1 | 0 | 0.0%"));
                assert!(!output.stdout.contains("grep | 1 | 1 | 100.0%"));
            },
        );
    }

    #[test]
    fn run_audit_attributes_unsupported_roots_to_passthrough_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".cx/cache/failures/passthrough")).unwrap();
        fs::write(
            home.join(".cx/cache/failures/passthrough/001.log"),
            "ssh failure",
        )
        .unwrap();
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
                let raw = OutputObservation::from_text("passthrough:ssh", "ssh: failed\n");
                record_invocation_with_context(
                    &InvocationRecord {
                        command: "passthrough ssh",
                        exit_code: 255,
                        raw: Some(&raw),
                        emitted: TextMetrics::from_text("ssh: failed\n"),
                    },
                    Some(&InvocationContext {
                        process: "ssh",
                        command: "ssh host false",
                        argv_json: r#"["cx","--","ssh","host","false"]"#,
                        emitted_response: Some("ssh: failed\n"),
                    }),
                )
                .unwrap();

                let output = run(5, CommandFilter::default(), AuditFormat::Text).unwrap();
                assert!(output.stdout.contains(
                    "No recorded output-bearing failure details lack artifact references"
                ));
                assert!(output.stdout.contains(
                    "ssh | 1 | 0 | 1 | 0 | 0 | 1 | ~/.cx/cache/failures/passthrough/001.log"
                ));
            },
        );
    }

    #[test]
    fn audit_artifact_coverage_summary_is_independent_of_display_limit() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        let home = temp.path().join("home");
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
                let gap_raw = OutputObservation::from_text("gap", "gap output\n");
                record_invocation_with_context_and_failure(
                    &InvocationRecord {
                        command: "gap run",
                        exit_code: 7,
                        raw: Some(&gap_raw),
                        emitted: TextMetrics::from_text("gap output\n"),
                    },
                    Some(&InvocationContext {
                        process: "gap",
                        command: "gap run",
                        argv_json: "[]",
                        emitted_response: Some("gap output\n"),
                    }),
                    Some(&FailureDetailRecord {
                        command_family: "gap run",
                        command_line: "gap run",
                        exit_code: 7,
                        cx_response: "gap output\n",
                        raw_source: Some("gap"),
                        raw_response: Some("gap output\n"),
                    }),
                )
                .unwrap();
                for process in ["unknown-a", "unknown-b"] {
                    let raw = OutputObservation::from_text(process, "unknown output\n");
                    record_invocation_with_context(
                        &InvocationRecord {
                            command: process,
                            exit_code: 8,
                            raw: Some(&raw),
                            emitted: TextMetrics::from_text("unknown output\n"),
                        },
                        Some(&InvocationContext {
                            process,
                            command: process,
                            argv_json: "[]",
                            emitted_response: Some("unknown output\n"),
                        }),
                    )
                    .unwrap();
                }

                let limited = run(1, CommandFilter::default(), AuditFormat::Json).unwrap();
                let complete = run(100, CommandFilter::default(), AuditFormat::Json).unwrap();
                let limited_json: serde_json::Value =
                    serde_json::from_str(&limited.stdout).unwrap();
                let complete_json: serde_json::Value =
                    serde_json::from_str(&complete.stdout).unwrap();

                assert_eq!(
                    limited_json["summary"]["artifact_coverage"],
                    complete_json["summary"]["artifact_coverage"]
                );
                assert_eq!(
                    limited_json["summary"]["artifact_coverage"]["roots_with_output_gaps"],
                    1
                );
                assert_eq!(
                    limited_json["summary"]["artifact_coverage"]["unknown_invocations"],
                    2
                );
                assert_eq!(
                    limited_json["failure_artifact_coverage"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(
                    complete_json["failure_artifact_coverage"]
                        .as_array()
                        .unwrap()
                        .len(),
                    3
                );
            },
        );
    }

    fn seed_audit_fixture() {
        let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(20));
        record_invocation(&InvocationRecord {
            command: "grep",
            exit_code: 2,
            raw: Some(&grep_raw),
            emitted: TextMetrics::from_text("match\n"),
        })
        .unwrap();
        let read_raw = OutputObservation::from_text("read source", &"line\n".repeat(5));
        record_invocation(&InvocationRecord {
            command: "read",
            exit_code: 0,
            raw: Some(&read_raw),
            emitted: TextMetrics::from_text("line\n"),
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
    }
}
