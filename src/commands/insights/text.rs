use anyhow::Result;

use crate::support::{
    insights::{
        self, CommandLevel, CommandReportInsight, CommandReportTotalInsight, CommandTotalInsight,
        CommandTotalSort, DailyInsight, InvocationInsight, OverallInsight, SavingsSort,
    },
    utils::truncate,
};

use super::failure_coverage::FailureFocus;
use super::format_utils::{
    div_floor, format_count, format_ratio, format_signed_count, signed_delta,
};

pub(super) fn no_data_message() -> Result<String> {
    Ok(format!(
        "cx insights: no data yet\nDatabase: {}\nEnable `record_invocations` with `cx insights settings --set record_invocations=true` to populate savings telemetry.",
        insights::insights_database_path()?.display()
    ))
}

pub(super) fn no_matching_data_message(filter: insights::CommandFilter<'_>) -> Result<String> {
    if filter.is_empty() {
        return no_data_message();
    }
    Ok(format!(
        "cx insights: no matching data\nDatabase: {}\n{}",
        insights::insights_database_path()?.display(),
        format_filter_line(filter).trim_end(),
    ))
}

pub(super) fn format_filter_line(filter: insights::CommandFilter<'_>) -> String {
    if filter.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(command_root) = filter.command_root {
        parts.push(format!("root={command_root}"));
    }
    if let Some(command) = filter.command {
        parts.push(format!("command={command}"));
    }
    format!("Filter: {}\n", parts.join(", "))
}

pub(super) fn format_overall(overall: &OverallInsight) -> String {
    format!(
        "Database: {}\nInvocations: {} ({} failures, {} expansions)\nSaved: {} chars, {} estimated tokens, {} lines, {} bytes\nExpanded: {} chars, {} estimated tokens, {} lines, {} bytes\nRaw observed: {} chars / {} estimated tokens\nEmitted: {} chars / {} estimated tokens\nNet token delta (emitted - raw): {}\nSavings ratio: {}",
        insights::insights_database_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "<unavailable>".to_string()),
        overall.invocations,
        overall.failures,
        overall.expansions,
        format_count(overall.saved.chars),
        format_count(overall.saved.tokens),
        format_count(overall.saved.lines),
        format_count(overall.saved.bytes),
        format_count(overall.expanded.chars),
        format_count(overall.expanded.tokens),
        format_count(overall.expanded.lines),
        format_count(overall.expanded.bytes),
        format_count(overall.raw.chars),
        format_count(overall.raw.tokens),
        format_count(overall.emitted.chars),
        format_count(overall.emitted.tokens),
        format_signed_count(signed_delta(overall.emitted.tokens, overall.raw.tokens)),
        format_ratio(overall.saved.chars, overall.raw.chars),
    )
}

pub(super) fn format_command_totals(totals: &[CommandTotalInsight]) -> String {
    if totals.is_empty() {
        return "(no command totals)\n".to_string();
    }
    let mut output = String::from(
        "command | invocations | failures | expansions | saved tokens | expanded tokens | net token delta | saved lines | saved chars | avg saved tokens | best saved tokens | best expanded tokens\n",
    );
    for total in totals {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            total.command,
            total.invocations,
            total.failures,
            total.expansions,
            format_count(total.saved.tokens),
            format_count(total.expanded.tokens),
            format_signed_count(signed_delta(total.emitted.tokens, total.raw.tokens)),
            format_count(total.saved.lines),
            format_count(total.saved.chars),
            format_count(div_floor(total.saved.tokens, total.invocations)),
            format_count(total.best_saved_tokens),
            format_count(total.best_expanded_tokens),
        ));
    }
    output.trim_end().to_string()
}

pub(super) fn format_failure_focus(focus: &[FailureFocus]) -> String {
    if focus.is_empty() {
        return "(no failed command totals)\n".to_string();
    }
    let mut output = String::from(
        "command | failures | details | unknown | output gaps | linked | retained | artifact tool | latest artifact\n",
    );
    for item in focus {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            item.total.command,
            format_count(item.coverage.failed_invocations),
            format_count(item.coverage.detail_rows),
            format_count(item.coverage.unknown_invocations),
            format_count(item.coverage.output_gap_detail_rows),
            format_count(item.coverage.artifact_linked_detail_rows),
            format_count(item.artifact_summary.count as u64),
            item.artifact_summary.tool_name,
            item.artifact_summary
                .latest_display_path
                .as_deref()
                .unwrap_or("-"),
        ));
    }
    let warnings = format_missing_artifact_warnings(focus);
    if !warnings.is_empty() {
        output.push('\n');
        output.push_str(&warnings);
    }
    output.trim_end().to_string()
}

pub(super) fn format_command_report_totals(totals: &[CommandReportTotalInsight]) -> String {
    if totals.is_empty() {
        return "(no command-quality report totals)\n".to_string();
    }
    let mut output = String::from(
        "command | total | open | resolved | native parity | not reproducible | denied | other\n",
    );
    for total in totals {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {}\n",
            total.command,
            format_count(total.reports),
            format_count(total.open),
            format_count(total.resolved),
            format_count(total.native_parity),
            format_count(total.not_reproducible),
            format_count(total.denied),
            format_count(total.other),
        ));
    }
    output.trim_end().to_string()
}

pub(super) fn format_command_reports(reports: &[CommandReportInsight]) -> String {
    if reports.is_empty() {
        return "(no recent command-quality reports)\n".to_string();
    }
    let mut output = String::from(
        "id | occurred_at_ms | root | family | status | denial reason | related | evidence | issue | command | report note | resolution | revision | status_updated_at_ms\n",
    );
    for report in reports {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            report.id,
            report.occurred_at_ms,
            report.command_root,
            report.command_family,
            report.status,
            if report.denial_reason.is_empty() {
                "-"
            } else {
                &report.denial_reason
            },
            report
                .related_report_id
                .map_or_else(|| "-".to_string(), |id| id.to_string()),
            report.evidence_kind,
            report.issue_kind,
            truncate(&report.command, 120),
            truncate(&report.note, 120),
            truncate(&report.resolution_note, 120),
            report.resolution_revision,
            report.status_updated_at_ms,
        ));
    }
    output.trim_end().to_string()
}

fn format_missing_artifact_warnings(focus: &[FailureFocus]) -> String {
    let mut output = String::new();
    for item in focus.iter().filter(|item| item.has_output_gap()) {
        if output.is_empty() {
            output.push_str("artifact coverage warnings:\n");
        }
        output.push_str(&format!(
            "warning: `{}` has {} output-bearing failure details without artifact references\n",
            item.total.command,
            format_count(item.coverage.output_gap_detail_rows),
        ));
    }
    for item in focus
        .iter()
        .filter(|item| item.coverage_unknown() && !item.has_output_gap())
    {
        if !output.is_empty() && !output.ends_with("\n\n") {
            output.push('\n');
        }
        if !output.contains("failure-detail coverage notes:\n") {
            output.push_str("failure-detail coverage notes:\n");
        }
        output.push_str(&format!(
            "unknown: `{}` has {} failed invocations without linked failure-detail evidence\n",
            item.total.command,
            format_count(item.coverage.unknown_invocations),
        ));
    }
    output
}

pub(super) fn format_invocations(invocations: &[InvocationInsight]) -> String {
    if invocations.is_empty() {
        return "(no invocation rows)\n".to_string();
    }
    let mut output = String::from(
        "id | occurred_at_ms | process | family | command | shape | exit | source | saved tokens | expanded tokens | expansion reason | saved lines | saved chars | savings ratio | compression | revision\n",
    );
    for invocation in invocations {
        let shape = if invocation.command_shape.is_empty() {
            "-"
        } else {
            &invocation.command_shape
        };
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1}% | {:.3} | {}\n",
            invocation.id,
            invocation.occurred_at_ms,
            invocation.process,
            invocation.command_family,
            invocation.command,
            truncate(shape, 120),
            invocation.exit_code,
            invocation.source,
            format_count(invocation.saved.tokens),
            format_count(invocation.expanded.tokens),
            if invocation.expansion_reason.is_empty() {
                "-"
            } else {
                &invocation.expansion_reason
            },
            format_count(invocation.saved.lines),
            format_count(invocation.saved.chars),
            invocation.savings_ratio * 100.0,
            invocation.compression_ratio,
            if invocation.binary_revision.is_empty() {
                "-"
            } else {
                &invocation.binary_revision
            },
        ));
    }
    output.trim_end().to_string()
}

pub(super) fn format_daily(days: &[DailyInsight]) -> String {
    if days.is_empty() {
        return "(no daily rows)\n".to_string();
    }
    let mut output = String::from(
        "day | invocations | failures | expansions | saved tokens | expanded tokens | net token delta | saved lines | saved chars\n",
    );
    for day in days {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            day.day,
            day.invocations,
            day.failures,
            day.expansions,
            format_count(day.saved.tokens),
            format_count(day.expanded.tokens),
            format_signed_count(signed_delta(day.expanded.tokens, day.saved.tokens)),
            format_count(day.saved.lines),
            format_count(day.saved.chars),
        ));
    }
    output.trim_end().to_string()
}

pub(super) fn command_total_sort_label(sort: CommandTotalSort) -> &'static str {
    match sort {
        CommandTotalSort::Tokens => "saved tokens",
        CommandTotalSort::Chars => "saved chars",
        CommandTotalSort::Lines => "saved lines",
        CommandTotalSort::Invocations => "invocations",
        CommandTotalSort::Failures => "failures",
    }
}

pub(super) fn command_level_label(level: CommandLevel) -> &'static str {
    match level {
        CommandLevel::Command => "commands",
        CommandLevel::Root => "command roots",
    }
}

pub(super) fn savings_sort_label(sort: SavingsSort) -> &'static str {
    match sort {
        SavingsSort::Tokens => "saved tokens",
        SavingsSort::Chars => "saved chars",
        SavingsSort::Lines => "saved lines",
    }
}
