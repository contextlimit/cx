use crate::support::insights::{SavingsDistributionInsight, TextMetrics};

use super::data::{ExportEvidence, ExportSnapshot, Recommendation};
use super::distribution_view::format_savings_distribution;
use super::format_utils::{
    div_floor, format_count, format_ratio, format_signed_count, signed_delta,
};
use super::text::format_invocations;

pub(super) const DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS: u64 = 200_000;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PresentationMetrics {
    pub(super) invocations: u64,
    pub(super) failures: u64,
    pub(super) expansions: u64,
    pub(super) failure_rate: f64,
    pub(super) raw: TextMetrics,
    pub(super) emitted: TextMetrics,
    pub(super) saved: TextMetrics,
    pub(super) expanded: TextMetrics,
    pub(super) net_token_delta: i64,
    pub(super) savings_ratio: f64,
    pub(super) average_saved_tokens: u64,
    pub(super) savings_distribution: SavingsDistributionInsight,
    pub(super) context_window_tokens: u64,
    pub(super) context_windows_saved: f64,
}

impl PresentationMetrics {
    pub(super) fn from_snapshot(snapshot: &ExportSnapshot, context_window_tokens: u64) -> Self {
        let context_window_tokens = context_window_tokens.max(1);
        let overall = &snapshot.overall;
        Self {
            invocations: overall.invocations,
            failures: overall.failures,
            expansions: overall.expansions,
            failure_rate: ratio(overall.failures, overall.invocations),
            raw: overall.raw,
            emitted: overall.emitted,
            saved: overall.saved,
            expanded: overall.expanded,
            net_token_delta: signed_delta(overall.emitted.tokens, overall.raw.tokens),
            savings_ratio: ratio(overall.saved.chars, overall.raw.chars),
            average_saved_tokens: div_floor(overall.saved.tokens, overall.invocations),
            savings_distribution: snapshot.savings_distribution,
            context_window_tokens,
            context_windows_saved: overall.saved.tokens as f64 / context_window_tokens as f64,
        }
    }
}

pub(super) fn format_presentation_metrics(metrics: &PresentationMetrics) -> String {
    format!(
        "Measured invocations: {}\nRaw output: {} bytes, {} chars, {} lines, {} estimated tokens\nEmitted output: {} bytes, {} chars, {} lines, {} estimated tokens\nSaved output: {} bytes, {} chars, {} lines, {} estimated tokens\nExpanded output: {} bytes, {} chars, {} lines, {} estimated tokens across {} invocations\nNet token delta (emitted - raw): {}\nAverage saved per invocation: {} estimated tokens\nSavings ratio: {}\nFailed invocations: {} ({})\nContext-window equivalents saved: {:.2} at {} tokens/window\n\nSavings distribution:\n{}",
        format_count(metrics.invocations),
        format_count(metrics.raw.bytes),
        format_count(metrics.raw.chars),
        format_count(metrics.raw.lines),
        format_count(metrics.raw.tokens),
        format_count(metrics.emitted.bytes),
        format_count(metrics.emitted.chars),
        format_count(metrics.emitted.lines),
        format_count(metrics.emitted.tokens),
        format_count(metrics.saved.bytes),
        format_count(metrics.saved.chars),
        format_count(metrics.saved.lines),
        format_count(metrics.saved.tokens),
        format_count(metrics.expanded.bytes),
        format_count(metrics.expanded.chars),
        format_count(metrics.expanded.lines),
        format_count(metrics.expanded.tokens),
        format_count(metrics.expansions),
        format_signed_count(metrics.net_token_delta),
        format_count(metrics.average_saved_tokens),
        format_ratio(metrics.saved.chars, metrics.raw.chars),
        format_count(metrics.failures),
        format_ratio(metrics.failures, metrics.invocations),
        metrics.context_windows_saved,
        format_count(metrics.context_window_tokens),
        format_savings_distribution(&metrics.savings_distribution),
    )
}

pub(super) fn format_impact_headlines(
    snapshot: &ExportSnapshot,
    context_window_tokens: u64,
) -> String {
    let overall = &snapshot.overall;
    let average_saved_tokens = div_floor(overall.saved.tokens, overall.invocations);
    let context_windows = overall.saved.tokens as f64 / context_window_tokens as f64;
    format!(
        "Database: {}\nInvocations measured: {}\nFailures observed: {}\nExpansions observed: {}\nTotal saved: {} estimated tokens, {} chars, {} lines\nTotal expanded: {} estimated tokens, {} chars, {} lines\nNet token delta (emitted - raw): {}\nAverage saved per invocation: {} estimated tokens\nSavings ratio: {}\nContext-window equivalents saved: {:.2} at {} tokens/window\n\nSavings distribution:\n{}",
        snapshot.database,
        format_count(overall.invocations),
        format_count(overall.failures),
        format_count(overall.expansions),
        format_count(overall.saved.tokens),
        format_count(overall.saved.chars),
        format_count(overall.saved.lines),
        format_count(overall.expanded.tokens),
        format_count(overall.expanded.chars),
        format_count(overall.expanded.lines),
        format_signed_count(signed_delta(overall.emitted.tokens, overall.raw.tokens)),
        format_count(average_saved_tokens),
        format_ratio(overall.saved.chars, overall.raw.chars),
        context_windows,
        format_count(context_window_tokens),
        format_savings_distribution(&snapshot.savings_distribution),
    )
}

pub(super) fn format_impact_command_concentration(snapshot: &ExportSnapshot) -> String {
    let mut output =
        String::from("rank | command | saved tokens | share of saved tokens | invocations\n");
    for (index, total) in snapshot.top_commands.iter().enumerate() {
        output.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            index + 1,
            total.command,
            format_count(total.saved.tokens),
            format_ratio(total.saved.tokens, snapshot.overall.saved.tokens),
            format_count(total.invocations),
        ));
    }
    output.trim_end().to_string()
}

pub(super) fn format_impact_bullets(
    snapshot: &ExportSnapshot,
    context_window_tokens: u64,
) -> String {
    let top = snapshot.top_commands.first();
    let largest = snapshot.largest_invocations.first();
    let latest_day = snapshot.daily_totals.first();
    let mut bullets = Vec::new();

    bullets.push(format!(
        "- CX saved {} estimated tokens across {} measured command invocations.",
        format_count(snapshot.overall.saved.tokens),
        format_count(snapshot.overall.invocations),
    ));
    bullets.push(format!(
        "- It removed {} of observed command-output characters before they reached the agent context.",
        format_ratio(snapshot.overall.saved.chars, snapshot.overall.raw.chars),
    ));
    bullets.push(format!(
        "- At {} tokens per context window, the measured savings equal {:.2} full windows.",
        format_count(context_window_tokens),
        snapshot.overall.saved.tokens as f64 / context_window_tokens as f64,
    ));
    bullets.push(format!(
        "- {} invocations expanded output by {} estimated tokens; the net token delta was {}.",
        format_count(snapshot.overall.expansions),
        format_count(snapshot.overall.expanded.tokens),
        format_signed_count(signed_delta(
            snapshot.overall.emitted.tokens,
            snapshot.overall.raw.tokens,
        )),
    ));
    bullets.push(format!(
        "- The top 10 saves account for {}; excluding them, the remaining invocations still saved {} estimated tokens.",
        format_ratio(
            snapshot.savings_distribution.top_ten_saved_tokens,
            snapshot.savings_distribution.total_saved_tokens,
        ),
        format_count(
            snapshot
                .savings_distribution
                .saved_tokens_excluding_top_ten(),
        ),
    ));

    if let Some(top) = top {
        bullets.push(format!(
            "- The top command family was `{}`, with {} saved tokens over {} invocations.",
            top.command,
            format_count(top.saved.tokens),
            format_count(top.invocations),
        ));
    }
    if let Some(largest) = largest {
        bullets.push(format!(
            "- The largest single invocation was `{}`, saving {} tokens and {} lines.",
            largest.command,
            format_count(largest.saved.tokens),
            format_count(largest.saved.lines),
        ));
    }
    if let Some(day) = latest_day {
        bullets.push(format!(
            "- The most recent daily bucket ({}) saved {} tokens across {} invocations.",
            day.day,
            format_count(day.saved.tokens),
            format_count(day.invocations),
        ));
    }

    bullets.join("\n")
}

pub(super) fn format_presentation_headlines(snapshot: &ExportSnapshot) -> String {
    let top_command = snapshot
        .top_commands
        .first()
        .map(|total| {
            format!(
                "`{}` saved {} tokens across {} invocations",
                total.command,
                format_count(total.saved.tokens),
                format_count(total.invocations),
            )
        })
        .unwrap_or_else(|| "No command family has enough data yet".to_string());
    let largest_save = snapshot
        .largest_invocations
        .first()
        .map(|invocation| {
            format!(
                "`{}` saved {} tokens and {} lines in one invocation",
                invocation.command,
                format_count(invocation.saved.tokens),
                format_count(invocation.saved.lines),
            )
        })
        .unwrap_or_else(|| "No single-invocation proof is available yet".to_string());

    format!(
        "Executive headlines:\n- CX saved {} estimated tokens, {} lines, and {} chars across {} measured invocations.\n- It removed {} of observed command-output characters before they reached the agent context.\n- {} invocations expanded output by {} estimated tokens; net token delta was {}.\n- The top 10 saves represent {}; savings outside those outliers total {} estimated tokens.\n- Top command family: {}.\n- Largest proof point: {}.",
        format_count(snapshot.overall.saved.tokens),
        format_count(snapshot.overall.saved.lines),
        format_count(snapshot.overall.saved.chars),
        format_count(snapshot.overall.invocations),
        format_ratio(snapshot.overall.saved.chars, snapshot.overall.raw.chars),
        format_count(snapshot.overall.expansions),
        format_count(snapshot.overall.expanded.tokens),
        format_signed_count(signed_delta(
            snapshot.overall.emitted.tokens,
            snapshot.overall.raw.tokens,
        )),
        format_ratio(
            snapshot.savings_distribution.top_ten_saved_tokens,
            snapshot.savings_distribution.total_saved_tokens,
        ),
        format_count(
            snapshot
                .savings_distribution
                .saved_tokens_excluding_top_ten(),
        ),
        top_command,
        largest_save,
    )
}

pub(super) fn format_presentation_slide_outline(
    snapshot: &ExportSnapshot,
    recommendations: &[Recommendation],
) -> String {
    let top_command = snapshot
        .top_commands
        .first()
        .map(|total| total.command.as_str())
        .unwrap_or("the top command family");
    let largest_command = snapshot
        .largest_invocations
        .first()
        .map(|invocation| invocation.command.as_str())
        .unwrap_or("the largest single save");
    let recommendation = recommendations
        .first()
        .map(|item| item.title.as_str())
        .unwrap_or("keep collecting real command evidence");

    [
        format!(
            "1. Measured problem: high-output local commands consumed {} raw estimated tokens.",
            format_count(snapshot.overall.raw.tokens),
        ),
        format!(
            "2. Measured result: CX saved {} estimated tokens; {} remain after excluding the top 10 saves.",
            format_count(snapshot.overall.saved.tokens),
            format_count(
                snapshot
                    .savings_distribution
                    .saved_tokens_excluding_top_ten(),
            ),
        ),
        format!("3. Workflow proof: `{top_command}` is the primary savings path."),
        format!("4. Memorable demo: `{largest_command}` is the largest single-save proof."),
        "5. Reliability posture: failures remain recoverable through artifact-backed health checks."
            .to_string(),
        format!("6. Next investment: {recommendation}."),
    ]
    .join("\n")
}

pub(super) fn format_presentation_recommendations(recommendations: &[Recommendation]) -> String {
    if recommendations.is_empty() {
        return "(no recommendations)\n".to_string();
    }
    let mut output = String::new();
    for (index, recommendation) in recommendations.iter().enumerate() {
        output.push_str(&format!(
            "{}. {}\n   Evidence: {}\n   Action: {}\n",
            index + 1,
            recommendation.title,
            recommendation.evidence,
            recommendation.action,
        ));
    }
    output.trim_end().to_string()
}

pub(super) fn format_presentation_operational_health(evidence: &ExportEvidence) -> String {
    let reports = evidence.command_report_status;
    let report_line = format!(
        "Quality reports: {} total; {} open; {} resolved; {} native parity; {} not reproducible; {} denied.",
        format_count(reports.total),
        format_count(reports.open),
        format_count(reports.resolved),
        format_count(reports.native_parity),
        format_count(reports.not_reproducible),
        format_count(reports.denied),
    );
    if evidence.snapshot.overall.failures == 0 {
        return format!(
            "No failed invocations recorded.\nFailure artifacts: no recovery evidence needed yet.\n{report_line}"
        );
    }

    let coverage = evidence.failure_coverage;
    let mut output = format!(
        "Failed invocations: {} across {} failing command families.\nFailure details: {}; unknown invocation coverage: {}; output-bearing artifact gaps: {} across {} families.\nArtifact-linked details: {}; currently retained families: {}; linked-but-pruned families: {}.",
        format_count(coverage.failed_invocations),
        format_count(coverage.failing_groups),
        format_count(coverage.detail_rows),
        format_count(coverage.unknown_invocations),
        format_count(coverage.output_gap_detail_rows),
        format_count(coverage.groups_with_output_gaps),
        format_count(coverage.artifact_linked_detail_rows),
        format_count(coverage.groups_with_retained_artifacts),
        format_count(coverage.groups_with_linked_but_pruned_artifacts),
    );

    if let Some(top_failure) = evidence.failure_focus.first() {
        output.push_str(&format!(
            "\nTop failure family: `{}` with {} failures, {} unknown, and {} output gaps; latest retained artifact: {}.",
            top_failure.total.command,
            format_count(top_failure.coverage.failed_invocations),
            format_count(top_failure.coverage.unknown_invocations),
            format_count(top_failure.coverage.output_gap_detail_rows),
            top_failure
                .artifact_summary
                .latest_display_path
                .as_deref()
                .unwrap_or("-"),
        ));
    }
    if coverage.groups_with_output_gaps > 0 {
        output.push_str(
            "\nUse `cx insights failures` to locate response-bearing failures without artifact references.",
        );
    }
    output.push('\n');
    output.push_str(&report_line);
    output
}

fn ratio(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

pub(super) fn format_report(evidence: &ExportEvidence) -> String {
    let snapshot = &evidence.snapshot;
    let mut output = String::new();
    output.push_str("Executive brief:\n");
    output.push_str(&format_report_executive_brief(snapshot));
    output.push_str("\n\nProof points:\n");
    output.push_str(&format_report_proof_points(snapshot));
    output.push_str("\n\nRecent activity:\n");
    output.push_str(&format_invocations(&snapshot.recent_invocations));
    output.push_str("\n\nRecommended next moves:\n");
    output.push_str(&format_presentation_recommendations(
        &evidence.recommendations,
    ));
    output.push_str("\n\nOperator commands:\n");
    output.push_str(&format_report_operator_commands(evidence.limit));
    output
}

fn format_report_executive_brief(snapshot: &ExportSnapshot) -> String {
    format!(
        "- {} measured invocations saved {} estimated tokens, {} lines, and {} chars.\n- CX removed {} of observed command-output characters before they reached agent context.\n- Savings outside the top 10 invocations total {} estimated tokens.\n- Failure rate: {} failed invocations out of {}.",
        format_count(snapshot.overall.invocations),
        format_count(snapshot.overall.saved.tokens),
        format_count(snapshot.overall.saved.lines),
        format_count(snapshot.overall.saved.chars),
        format_ratio(snapshot.overall.saved.chars, snapshot.overall.raw.chars),
        format_count(
            snapshot
                .savings_distribution
                .saved_tokens_excluding_top_ten(),
        ),
        format_count(snapshot.overall.failures),
        format_count(snapshot.overall.invocations),
    )
}

fn format_report_proof_points(snapshot: &ExportSnapshot) -> String {
    let top = snapshot.top_commands.first();
    let largest = snapshot.largest_invocations.first();
    let recent = snapshot.recent_invocations.first();
    let mut output = String::new();
    if let Some(top) = top {
        output.push_str(&format!(
            "- Top family: `{}` saved {} tokens across {} invocations.\n",
            top.command,
            format_count(top.saved.tokens),
            format_count(top.invocations),
        ));
    }
    if let Some(largest) = largest {
        output.push_str(&format!(
            "- Largest single save: `{}` saved {} tokens and {} lines.\n",
            largest.command,
            format_count(largest.saved.tokens),
            format_count(largest.saved.lines),
        ));
    }
    if let Some(recent) = recent {
        output.push_str(&format!(
            "- Most recent measured command: `{}` saved {} tokens with exit code {}.\n",
            recent.command,
            format_count(recent.saved.tokens),
            recent.exit_code,
        ));
    }
    output.trim_end().to_string()
}

fn format_report_operator_commands(limit: usize) -> String {
    let limit = limit.clamp(1, 100);
    format!(
        "cx insights report --limit {limit}\ncx insights export --format json --limit {limit}\ncx insights recent --limit {limit}\ncx insights failures --limit {limit}",
    )
}

pub(super) fn format_presentation_demo_commands(limit: usize) -> String {
    let limit = limit.clamp(1, 100);
    format!(
        "cx insights impact --limit {limit} --context-window-tokens {}\ncx insights recommend --limit {limit}\ncx insights failures --limit {limit}\ncx insights export --format json --limit {limit}",
        DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS,
    )
}
