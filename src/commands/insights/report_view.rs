use crate::support::insights::{CommandReportInsight, CommandReportStatusSummary};

pub(crate) fn report_json(report: &CommandReportInsight) -> serde_json::Value {
    serde_json::json!({
        "id": report.id,
        "occurred_at_ms": report.occurred_at_ms,
        "command_root": report.command_root,
        "command": report.command,
        "command_family": report.command_family,
        "reported_command": report.command,
        "issue_kind": report.issue_kind,
        "note": report.note,
        "status": report.status,
        "resolution_note": report.resolution_note,
        "resolution_revision": report.resolution_revision,
        "denial_reason": report.denial_reason,
        "related_report_id": report.related_report_id,
        "status_updated_at_ms": report.status_updated_at_ms,
        "invocation_id": report.invocation_id,
        "evidence_kind": report.evidence_kind,
        "cx_response_recorded": !report.cx_response.is_empty(),
        "native_response_recorded": !report.native_response.is_empty(),
        "artifact_ref": report.artifact_ref,
    })
}

pub(crate) fn status_summary_json(summary: &CommandReportStatusSummary) -> serde_json::Value {
    serde_json::json!({
        "total": summary.total,
        "open": summary.open,
        "resolved": summary.resolved,
        "native_parity": summary.native_parity,
        "not_reproducible": summary.not_reproducible,
        "denied": summary.denied,
        "closed": summary.closed(),
        "other": summary.other,
    })
}
