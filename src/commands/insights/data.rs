use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::support::insights::{
    self, CommandLevel, CommandOpportunityInsight, CommandReportDenialReasonSummary,
    CommandReportInsight, CommandReportStatusSummary, CommandReportTotalInsight,
    CommandTotalInsight, CommandTotalSort, DailyInsight, FailureArtifactInsight, InvocationInsight,
    OverallInsight, RoutingDecisionInsight, RoutingDecisionSummary, RoutingDecisionTotalInsight,
    SavingsDistributionInsight, SavingsSort,
};

use super::failure_coverage::{load_failure_coverage, FailureCoverageSummary, FailureFocus};
use super::format_utils::{div_floor, format_count, format_ratio};

pub(super) struct ExportEvidence {
    pub(super) snapshot: ExportSnapshot,
    pub(super) recommendations: Vec<Recommendation>,
    pub(super) failure_focus: Vec<FailureFocus>,
    pub(super) failure_coverage: FailureCoverageSummary,
    pub(super) command_report_totals: Vec<CommandReportTotalInsight>,
    pub(super) command_report_status: CommandReportStatusSummary,
    pub(super) command_report_denial_reasons: CommandReportDenialReasonSummary,
    pub(super) recent_command_reports: Vec<CommandReportInsight>,
    pub(super) recent_failure_artifacts: Vec<FailureArtifactInsight>,
    pub(super) passthrough_opportunities: Vec<CommandOpportunityInsight>,
    pub(super) routing_summary: RoutingDecisionSummary,
    pub(super) routing_decision_totals: Vec<RoutingDecisionTotalInsight>,
    pub(super) recent_routing_decisions: Vec<RoutingDecisionInsight>,
    pub(super) filter: FilterSummary,
    pub(super) limit: usize,
    pub(super) generated_at_ms: u64,
}

impl ExportEvidence {
    pub(super) fn load(limit: usize, filter: insights::CommandFilter<'_>) -> Result<Self> {
        let snapshot = ExportSnapshot::load(limit, filter)?;
        let command_report_totals = insights::command_report_totals_at_level(
            CommandLevel::Command,
            limit.clamp(1, 100),
            filter,
        )?;
        let recent_command_reports =
            insights::recent_command_reports_filtered(limit.clamp(1, 100), filter)?;
        let command_report_status = insights::command_report_status_summary(filter)?;
        let command_report_denial_reasons = insights::command_report_denial_reason_summary(filter)?;
        let recent_failure_artifacts = insights::recent_failure_artifacts(limit.clamp(1, 100))?;
        let passthrough_opportunities = insights::command_opportunities(limit.clamp(1, 100))?;
        let routing_summary = insights::routing_decision_summary(filter)?;
        let routing_decision_totals =
            insights::routing_decision_totals(limit.clamp(1, 100), filter)?;
        let recent_routing_decisions =
            insights::recent_routing_decisions(limit.clamp(1, 100), filter)?;
        if snapshot.no_data() {
            return Ok(Self {
                snapshot,
                recommendations: Vec::new(),
                failure_focus: Vec::new(),
                failure_coverage: FailureCoverageSummary::default(),
                command_report_totals,
                command_report_status,
                command_report_denial_reasons,
                recent_command_reports,
                recent_failure_artifacts,
                passthrough_opportunities,
                routing_summary,
                routing_decision_totals,
                recent_routing_decisions,
                filter: FilterSummary::from_filter(filter),
                limit,
                generated_at_ms: now_ms(),
            });
        }

        let analysis = RecommendationAnalysis::load_with_summary(
            filter,
            snapshot.overall.clone(),
            snapshot.savings_distribution,
        )?;
        let recommendations = build_recommendations(&analysis, limit.clamp(1, 12));
        let mut failure_focus = analysis.failure_focus;
        failure_focus.truncate(limit.clamp(1, 100));
        Ok(Self {
            snapshot,
            recommendations,
            failure_focus,
            failure_coverage: analysis.failure_coverage,
            command_report_totals,
            command_report_status,
            command_report_denial_reasons,
            recent_command_reports,
            recent_failure_artifacts,
            passthrough_opportunities,
            routing_summary,
            routing_decision_totals,
            recent_routing_decisions,
            filter: FilterSummary::from_filter(filter),
            limit,
            generated_at_ms: now_ms(),
        })
    }
}

pub(super) struct ExportSnapshot {
    pub(super) database: String,
    pub(super) overall: OverallInsight,
    pub(super) savings_distribution: SavingsDistributionInsight,
    pub(super) top_roots: Vec<CommandTotalInsight>,
    pub(super) top_commands: Vec<CommandTotalInsight>,
    pub(super) largest_invocations: Vec<InvocationInsight>,
    pub(super) recent_invocations: Vec<InvocationInsight>,
    pub(super) daily_totals: Vec<DailyInsight>,
}

impl ExportSnapshot {
    pub(super) fn load(limit: usize, filter: insights::CommandFilter<'_>) -> Result<Self> {
        Ok(Self {
            database: insights::insights_database_path()?.display().to_string(),
            overall: insights::overall_totals_filtered(filter)?,
            savings_distribution: insights::savings_distribution_filtered(filter)?,
            top_roots: insights::command_totals_at_level_filtered(
                CommandLevel::Root,
                CommandTotalSort::Tokens,
                limit,
                filter,
            )?,
            top_commands: insights::command_totals_at_level_filtered(
                CommandLevel::Command,
                CommandTotalSort::Tokens,
                limit,
                filter,
            )?,
            largest_invocations: insights::largest_invocations_filtered(
                SavingsSort::Tokens,
                limit,
                filter,
            )?,
            recent_invocations: insights::recent_invocations_filtered(limit, filter)?,
            daily_totals: insights::daily_totals_filtered(limit, filter)?,
        })
    }

    pub(super) fn no_data(&self) -> bool {
        self.overall.invocations == 0
    }
}

pub(super) struct RecommendationAnalysis {
    pub(super) database: String,
    pub(super) overall: OverallInsight,
    pub(super) savings_distribution: SavingsDistributionInsight,
    pub(super) by_tokens: Vec<CommandTotalInsight>,
    pub(super) by_invocations: Vec<CommandTotalInsight>,
    pub(super) by_failures: Vec<CommandTotalInsight>,
    pub(super) failure_focus: Vec<FailureFocus>,
    pub(super) failure_coverage: FailureCoverageSummary,
    pub(super) largest_invocations: Vec<InvocationInsight>,
}

impl RecommendationAnalysis {
    pub(super) fn load(filter: insights::CommandFilter<'_>) -> Result<Self> {
        Self::load_with_summary(
            filter,
            insights::overall_totals_filtered(filter)?,
            insights::savings_distribution_filtered(filter)?,
        )
    }

    fn load_with_summary(
        filter: insights::CommandFilter<'_>,
        overall: OverallInsight,
        savings_distribution: SavingsDistributionInsight,
    ) -> Result<Self> {
        let by_failures = insights::command_totals_at_level_filtered(
            CommandLevel::Command,
            CommandTotalSort::Failures,
            100,
            filter,
        )?;
        let failure_coverage = load_failure_coverage(CommandLevel::Command, filter)?;
        Ok(Self {
            database: insights::insights_database_path()?.display().to_string(),
            overall,
            savings_distribution,
            by_tokens: insights::command_totals_at_level_filtered(
                CommandLevel::Command,
                CommandTotalSort::Tokens,
                100,
                filter,
            )?,
            by_invocations: insights::command_totals_at_level_filtered(
                CommandLevel::Command,
                CommandTotalSort::Invocations,
                100,
                filter,
            )?,
            by_failures,
            failure_focus: failure_coverage.rows,
            failure_coverage: failure_coverage.summary,
            largest_invocations: insights::largest_invocations_filtered(
                SavingsSort::Tokens,
                10,
                filter,
            )?,
        })
    }
}

pub(super) struct Recommendation {
    pub(super) title: String,
    pub(super) command: String,
    pub(super) evidence: String,
    pub(super) action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct FilterSummary {
    pub(super) command_root: Option<String>,
    pub(super) command: Option<String>,
}

impl FilterSummary {
    fn from_filter(filter: insights::CommandFilter<'_>) -> Self {
        Self {
            command_root: filter.command_root.map(ToString::to_string),
            command: filter.command.map(ToString::to_string),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.command_root.is_none() && self.command.is_none()
    }
}

pub(super) fn build_recommendations(
    analysis: &RecommendationAnalysis,
    limit: usize,
) -> Vec<Recommendation> {
    let mut recommendations = Vec::new();
    push_primary_savings_recommendation(&mut recommendations, analysis);
    push_missing_artifact_recommendation(&mut recommendations, analysis);
    push_unknown_failure_detail_recommendation(&mut recommendations, analysis);
    push_failure_heavy_recommendation(&mut recommendations, analysis);
    push_demo_command_recommendation(&mut recommendations, analysis);
    push_frequency_recommendation(&mut recommendations, analysis);
    push_largest_save_recommendation(&mut recommendations, analysis);

    recommendations.truncate(limit);
    recommendations
}

fn push_primary_savings_recommendation(
    recommendations: &mut Vec<Recommendation>,
    analysis: &RecommendationAnalysis,
) {
    if let Some(total) = analysis
        .by_tokens
        .iter()
        .find(|total| total.saved.tokens > 0)
    {
        push_recommendation(
            recommendations,
            Recommendation {
                title: format!("Protect `{}` as the primary savings path", total.command),
                command: total.command.clone(),
                evidence: format!(
                    "{} saved tokens, {} of all saved tokens, {} invocations.",
                    format_count(total.saved.tokens),
                    format_ratio(total.saved.tokens, analysis.overall.saved.tokens),
                    format_count(total.invocations),
                ),
                action:
                    "Keep output-metric tests, installed smokes, and direct execution semantics tight before changing this path."
                        .to_string(),
            },
        );
    }
}

fn push_missing_artifact_recommendation(
    recommendations: &mut Vec<Recommendation>,
    analysis: &RecommendationAnalysis,
) {
    if let Some(item) = analysis
        .failure_focus
        .iter()
        .find(|item| item.has_output_gap())
    {
        push_recommendation(
            recommendations,
            Recommendation {
                title: format!("Repair artifact coverage for `{}`", item.total.command),
                command: item.total.command.clone(),
                evidence: format!(
                    "{} output-bearing failure details lack artifact references; {} failed invocations have unknown detail coverage.",
                    format_count(item.coverage.output_gap_detail_rows),
                    format_count(item.coverage.unknown_invocations),
                ),
                action:
                    "Inspect the command failure path and artifact-linking helper for the uncovered response-bearing cases."
                        .to_string(),
            },
        );
    }
}

fn push_unknown_failure_detail_recommendation(
    recommendations: &mut Vec<Recommendation>,
    analysis: &RecommendationAnalysis,
) {
    if let Some(item) = analysis
        .failure_focus
        .iter()
        .find(|item| item.coverage_unknown() && !item.has_output_gap())
    {
        push_recommendation(
            recommendations,
            Recommendation {
                title: format!(
                    "Improve failure-detail coverage for `{}`",
                    item.total.command
                ),
                command: item.total.command.clone(),
                evidence: format!(
                    "{} of {} failed invocations have no linked failure-detail evidence.",
                    format_count(item.coverage.unknown_invocations),
                    format_count(item.coverage.failed_invocations),
                ),
                action:
                    "Check whether response recording was disabled, unavailable in older telemetry, or bypassed by this command path."
                        .to_string(),
            },
        );
    }
}

fn push_failure_heavy_recommendation(
    recommendations: &mut Vec<Recommendation>,
    analysis: &RecommendationAnalysis,
) {
    if let Some(total) = analysis.by_failures.iter().find(|total| total.failures > 0) {
        push_recommendation(
            recommendations,
            Recommendation {
                title: format!("Investigate failure-heavy `{}` usage", total.command),
                command: total.command.clone(),
                evidence: format!(
                    "{} failures across {} invocations while still saving {} tokens.",
                    format_count(total.failures),
                    format_count(total.invocations),
                    format_count(total.saved.tokens),
                ),
                action:
                    "Review failure artifacts and parser edge cases so compact summaries stay truthful when tools fail."
                        .to_string(),
            },
        );
    }
}

fn push_demo_command_recommendation(
    recommendations: &mut Vec<Recommendation>,
    analysis: &RecommendationAnalysis,
) {
    if let Some(total) = best_average_saver(&analysis.by_tokens) {
        push_recommendation(
            recommendations,
            Recommendation {
                title: format!("Use `{}` as the strongest demo command", total.command),
                command: total.command.clone(),
                evidence: format!(
                    "Average savings are {} tokens per invocation; best single event saved {} tokens. The overall top-10 concentration is {}.",
                    format_count(div_floor(total.saved.tokens, total.invocations)),
                    format_count(total.best_saved_tokens),
                    format_ratio(
                        analysis.savings_distribution.top_ten_saved_tokens,
                        analysis.savings_distribution.total_saved_tokens,
                    ),
                ),
                action:
                    "Lead with the before/after example, then show p50/p95 and excluded-top-10 totals so the demo is not presented as a baseline."
                        .to_string(),
            },
        );
    }
}

fn push_frequency_recommendation(
    recommendations: &mut Vec<Recommendation>,
    analysis: &RecommendationAnalysis,
) {
    if let Some(total) = analysis.by_invocations.first() {
        push_recommendation(
            recommendations,
            Recommendation {
                title: format!("Tune the highest-frequency `{}` workflow", total.command),
                command: total.command.clone(),
                evidence: format!(
                    "{} invocations with {} average saved tokens per call.",
                    format_count(total.invocations),
                    format_count(div_floor(total.saved.tokens, total.invocations)),
                ),
                action:
                    "Prefer ergonomic defaults and clear recovery hints here because small changes compound quickly."
                        .to_string(),
            },
        );
    }
}

fn push_largest_save_recommendation(
    recommendations: &mut Vec<Recommendation>,
    analysis: &RecommendationAnalysis,
) {
    if let Some(invocation) = analysis.largest_invocations.iter().find(|invocation| {
        invocation.saved.tokens > 0 || invocation.saved.lines > 0 || invocation.saved.chars > 0
    }) {
        push_recommendation(
            recommendations,
            Recommendation {
                title: "Keep a concrete largest-save proof in the pitch".to_string(),
                command: invocation.command.clone(),
                evidence: format!(
                    "One invocation saved {} tokens and {} lines with a {:.1}% savings ratio; invocations outside the top 10 still saved {} tokens.",
                    format_count(invocation.saved.tokens),
                    format_count(invocation.saved.lines),
                    invocation.savings_ratio * 100.0,
                    format_count(
                        analysis
                            .savings_distribution
                            .saved_tokens_excluding_top_ten(),
                    ),
                ),
                action:
                    "Pair this memorable example with the top-10 concentration and excluded-top-10 totals so one outlier never carries the whole claim."
                        .to_string(),
            },
        );
    }
}

fn best_average_saver(totals: &[CommandTotalInsight]) -> Option<&CommandTotalInsight> {
    totals
        .iter()
        .filter(|total| total.invocations > 0 && total.saved.tokens > 0)
        .max_by_key(|total| {
            (
                div_floor(total.saved.tokens, total.invocations),
                total.saved.tokens,
            )
        })
}

fn push_recommendation(recommendations: &mut Vec<Recommendation>, recommendation: Recommendation) {
    let already_present = recommendations
        .iter()
        .any(|item| item.title == recommendation.title);
    if !already_present {
        recommendations.push(recommendation);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
