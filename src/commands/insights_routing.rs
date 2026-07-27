use anyhow::Result;

use crate::support::{
    insights::{
        self, CommandFilter, RoutingDecisionInsight, RoutingDecisionSummary,
        RoutingDecisionTotalInsight,
    },
    runner::ProxyOutcome,
};

use super::csv_view::{push_metric_row, CsvMetricRow};
use super::text::format_filter_line;

pub fn run(limit: usize, filter: CommandFilter<'_>) -> Result<ProxyOutcome> {
    let limit = limit.clamp(1, 100);
    let summary = insights::routing_decision_summary(filter)?;
    let totals = insights::routing_decision_totals(limit, filter)?;
    let recent = insights::recent_routing_decisions(limit, filter)?;
    let mut output = String::from("cx insights: routing rejections\n");
    output.push_str(&format!(
        "Database: {}\n",
        insights::insights_database_path()?.display()
    ));
    output.push_str(&format_filter_line(filter));
    output.push_str(&format!(
        "Rejected: {} | passthrough eligible: {} | passthrough disabled: {} | CX-owned errors: {}\n",
        summary.rejections,
        summary.passthrough_eligible,
        summary.passthrough_disabled,
        summary.cx_owned_errors,
    ));
    if recent.is_empty() {
        output.push_str("No routing rejections recorded.");
        return Ok(ProxyOutcome::success(output));
    }

    output.push_str("\nTotals:\nroot | family | reason | error kind | decisions | latest ms\n");
    for total in totals {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {}\n",
            total.command_root,
            total.command_family,
            total.reason,
            total.error_kind,
            total.decisions,
            total.latest_at_ms,
        ));
    }
    output.push_str(
        "\nRecent:\nid | time ms | root | family | reason | error kind | eligible | enabled | auto | command\n",
    );
    for decision in recent {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            decision.id,
            decision.occurred_at_ms,
            decision.process,
            decision.command_family,
            decision.reason,
            decision.error_kind,
            yes_no(decision.passthrough_eligible),
            yes_no(decision.passthrough_enabled),
            yes_no(decision.explicit_auto),
            truncate(&decision.command, 180),
        ));
    }
    Ok(ProxyOutcome::success(output.trim_end().to_string()))
}

pub(super) fn routing_decision_json(item: &RoutingDecisionInsight) -> serde_json::Value {
    serde_json::json!({
        "id": item.id,
        "occurred_at_ms": item.occurred_at_ms,
        "process": item.process,
        "command_root": item.process,
        "command_family": item.command_family,
        "command": item.command,
        "argv": serde_json::from_str::<serde_json::Value>(&item.argv_json)
            .unwrap_or_else(|_| serde_json::json!([])),
        "command_shape": item.command_shape,
        "command_shape_hash": item.command_shape_hash,
        "decision": item.decision,
        "reason": item.reason,
        "error_kind": item.error_kind,
        "explicit_auto": item.explicit_auto,
        "passthrough_eligible": item.passthrough_eligible,
        "passthrough_enabled": item.passthrough_enabled,
        "thread_id": item.thread_id,
        "plan_title": item.plan_title,
        "plan_project_id": item.plan_project_id,
        "plan_folder_id": item.plan_folder_id,
        "cwd": item.cwd,
        "git_root": item.git_root,
    })
}

pub(super) fn routing_total_json(item: &RoutingDecisionTotalInsight) -> serde_json::Value {
    serde_json::json!({
        "id": format!("{}:{}:{}:{}", item.command_root, item.command_family, item.reason, item.error_kind),
        "command_root": item.command_root,
        "command_family": item.command_family,
        "reason": item.reason,
        "error_kind": item.error_kind,
        "decisions": item.decisions,
        "latest_at_ms": item.latest_at_ms,
    })
}

pub(super) fn routing_summary_json(summary: &RoutingDecisionSummary) -> serde_json::Value {
    serde_json::json!({
        "rejections": summary.rejections,
        "passthrough_eligible": summary.passthrough_eligible,
        "passthrough_disabled": summary.passthrough_disabled,
        "cx_owned_errors": summary.cx_owned_errors,
        "latest_at_ms": summary.latest_at_ms,
    })
}

pub(super) fn push_routing_csv_rows(
    output: &mut String,
    summary: &RoutingDecisionSummary,
    totals: &[RoutingDecisionTotalInsight],
    recent: &[RoutingDecisionInsight],
) {
    for (metric, value) in [
        ("rejections", summary.rejections),
        ("passthrough_eligible", summary.passthrough_eligible),
        ("passthrough_disabled", summary.passthrough_disabled),
        ("cx_owned_errors", summary.cx_owned_errors),
        ("latest_at_ms", summary.latest_at_ms),
    ] {
        push_metric_row(output, CsvMetricRow::new("routing_summary", metric, value));
    }

    for (index, total) in totals.iter().enumerate() {
        let base = CsvMetricRow::new("routing_decision_totals", "", "")
            .rank(index + 1)
            .process(&total.command_root)
            .command_family(&total.command_family)
            .source(&format!("parser:{}", total.error_kind));
        push_metric_row(output, base.clone().metric("decisions", total.decisions));
        push_metric_row(output, base.clone().metric("reason", &total.reason));
        push_metric_row(output, base.metric("latest_at_ms", total.latest_at_ms));
    }

    for (index, item) in recent.iter().enumerate() {
        let base = CsvMetricRow::new("recent_routing_decisions", "", "")
            .rank(index + 1)
            .process(&item.process)
            .command_family(&item.command_family)
            .command(&item.command)
            .invocation_id(item.id)
            .source(&format!("parser:{}", item.error_kind))
            .exit_code(2)
            .argv_json(&item.argv_json)
            .command_shape(&item.command_shape)
            .command_shape_hash(&item.command_shape_hash);
        for (metric, value) in [
            ("reason", item.reason.clone()),
            ("explicit_auto", item.explicit_auto.to_string()),
            (
                "passthrough_eligible",
                item.passthrough_eligible.to_string(),
            ),
            ("passthrough_enabled", item.passthrough_enabled.to_string()),
        ] {
            push_metric_row(output, base.clone().metric(metric, value));
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let marker = "...";
    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(marker.len()))
        .collect::<String>();
    output.push_str(marker);
    output
}
