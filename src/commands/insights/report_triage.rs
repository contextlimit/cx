use anyhow::Result;

use crate::support::{
    insights::{
        self, CommandReportDenialReasonSummary, CommandReportStatusSummary,
        CommandReportTriageProposal, CommandReportTriageReceipt,
    },
    runner::ProxyOutcome,
    utils::truncate,
};

const TRIAGE_SCHEMA_NAME: &str = "cx-insights-report-triage";
const TRIAGE_SCHEMA_VERSION: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportTriageFormat {
    Text,
    Json,
}

pub fn run(apply: bool, format: ReportTriageFormat, limit: usize) -> Result<ProxyOutcome> {
    let receipt = insights::triage_command_reports(apply)?;
    let output = match format {
        ReportTriageFormat::Text => format_text(&receipt, limit.clamp(1, 100))?,
        ReportTriageFormat::Json => {
            serde_json::to_string_pretty(&format_json(&receipt, limit.clamp(1, 100))?)?
        }
    };
    Ok(ProxyOutcome::success(output))
}

fn format_text(receipt: &CommandReportTriageReceipt, limit: usize) -> Result<String> {
    let mut output = String::from("cx insights: report triage\n");
    output.push_str(&format!(
        "Database: {}\nMode: {}\n",
        insights::insights_database_path()?.display(),
        if receipt.applied {
            "applied"
        } else {
            "dry-run"
        },
    ));
    output.push_str(&format_status("Current lifecycle", receipt.current));
    output.push('\n');
    output.push_str(&format_reasons(receipt.denial_reasons));
    output.push('\n');
    output.push_str(&format_status("Projected lifecycle", receipt.projected));
    if receipt.applied {
        output.push('\n');
        output.push_str(&format!(
            "Applied: {} | skipped after recheck: {}\n",
            receipt.applied_count, receipt.skipped_count
        ));
        output.push_str(&format_status("Final lifecycle", receipt.final_status));
    }
    if receipt.proposals.is_empty() {
        output.push_str("\nNo conservative triage proposals.");
        return Ok(output);
    }
    output.push_str("\n\nProposals:\n");
    output.push_str("id | reason | related | rule | evidence | root | family | command\n");
    for proposal in receipt.proposals.iter().take(limit) {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {}\n",
            proposal.report_id,
            proposal.denial_reason,
            proposal
                .related_report_id
                .map_or_else(|| "-".to_string(), |id| id.to_string()),
            proposal.policy_rule_id,
            proposal.evidence_kind,
            proposal.command_root,
            proposal.command_family,
            truncate(&proposal.command, 160),
        ));
    }
    if receipt.proposals.len() > limit {
        output.push_str(&format!(
            "... {} additional proposals omitted by --limit\n",
            receipt.proposals.len() - limit
        ));
    }
    if !receipt.applied {
        output.push_str(
            "\nNo changes were written. Re-run with --apply after reviewing the proposal.",
        );
    }
    Ok(output.trim_end().to_string())
}

fn format_status(label: &str, status: CommandReportStatusSummary) -> String {
    format!(
        "{label}: {} total | {} open | {} resolved | {} native parity | {} not reproducible | {} denied | {} other",
        status.total,
        status.open,
        status.resolved,
        status.native_parity,
        status.not_reproducible,
        status.denied,
        status.other,
    )
}

fn format_reasons(summary: CommandReportDenialReasonSummary) -> String {
    format!(
        "Proposals: {} total | {} duplicate | {} invalid | {} insufficient evidence | {} obsolete | {} unsupported | {} low value | {} other",
        summary.total,
        summary.duplicate,
        summary.invalid,
        summary.insufficient_evidence,
        summary.obsolete,
        summary.unsupported,
        summary.low_value,
        summary.other,
    )
}

fn format_json(receipt: &CommandReportTriageReceipt, limit: usize) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "schema_name": TRIAGE_SCHEMA_NAME,
        "schema_version": TRIAGE_SCHEMA_VERSION,
        "database": insights::insights_database_path()?.display().to_string(),
        "mode": if receipt.applied { "applied" } else { "dry-run" },
        "applied": receipt.applied,
        "limit": limit,
        "current_lifecycle": super::report_view::status_summary_json(&receipt.current),
        "proposal_summary": denial_reason_json(receipt.denial_reasons),
        "projected_lifecycle": super::report_view::status_summary_json(&receipt.projected),
        "final_lifecycle": super::report_view::status_summary_json(&receipt.final_status),
        "applied_count": receipt.applied_count,
        "skipped_count": receipt.skipped_count,
        "proposal_count": receipt.proposals.len(),
        "proposal_rows_truncated": receipt.proposals.len().saturating_sub(limit),
        "proposals": receipt.proposals.iter().take(limit).map(proposal_json).collect::<Vec<_>>(),
        "policy": {
            "revision": "r132",
            "response_backed_reports_protected": true,
            "invocation_linked_reports_protected": true,
            "artifact_linked_reports_protected": true,
            "specific_note_reports_protected": true,
            "rules": [
                {
                    "id": "exact-duplicate-v1",
                    "reason": "duplicate",
                    "predicate": "exact root, family, command, and issue kind; non-canonical row has no CX or native response",
                },
                {
                    "id": "bare-command-without-actionable-evidence-v2",
                    "reason": "invalid",
                    "predicate": "trimmed command is empty, sh, bash, --, or cx and no response, invocation, or artifact evidence is recorded",
                },
                {
                    "id": "generic-note-without-actionable-evidence-v2",
                    "reason": "insufficient_evidence",
                    "predicate": "no response, invocation, or artifact evidence is recorded and the report note exactly matches the generic placeholder",
                }
            ]
        }
    }))
}

fn denial_reason_json(summary: CommandReportDenialReasonSummary) -> serde_json::Value {
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

fn proposal_json(proposal: &CommandReportTriageProposal) -> serde_json::Value {
    serde_json::json!({
        "report_id": proposal.report_id,
        "denial_reason": proposal.denial_reason.as_str(),
        "related_report_id": proposal.related_report_id,
        "policy_rule_id": proposal.policy_rule_id,
        "command_root": proposal.command_root,
        "command_family": proposal.command_family,
        "command": proposal.command,
        "evidence_kind": proposal.evidence_kind,
        "cx_response_recorded": proposal.cx_response_recorded,
        "native_response_recorded": proposal.native_response_recorded,
        "invocation_recorded": proposal.invocation_recorded,
        "artifact_recorded": proposal.artifact_recorded,
        "actionable_evidence_recorded": proposal.actionable_evidence_recorded,
        "note": proposal.note,
    })
}
