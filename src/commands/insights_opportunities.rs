use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};

use crate::support::{
    insights::{self, CommandOpportunityInsight, OpportunityConfidence, OpportunityFilter},
    runner::ProxyOutcome,
};

use super::{
    csv_view::{push_metric_row, CsvMetricRow},
    export::{metrics_json, push_metric_group},
    format_utils::{div_floor, format_count},
};

pub fn run_opportunities(limit: usize) -> Result<ProxyOutcome> {
    render_opportunities(
        insights::command_opportunities(limit.clamp(1, 100))?,
        "all recorded time",
        OpportunityConfidence::Low,
    )
}

pub fn run_opportunities_filtered(
    limit: usize,
    since: &str,
    minimum_confidence: OpportunityConfidence,
) -> Result<ProxyOutcome> {
    let (since_ms, window_label) = parse_since(since)?;
    let opportunities = insights::command_opportunities_filtered(
        limit.clamp(1, 100),
        OpportunityFilter {
            since_ms,
            minimum_confidence,
        },
    )?;
    render_opportunities(opportunities, &window_label, minimum_confidence)
}

fn render_opportunities(
    opportunities: Vec<CommandOpportunityInsight>,
    window_label: &str,
    minimum_confidence: OpportunityConfidence,
) -> Result<ProxyOutcome> {
    let mut output = String::from("cx insights: passthrough opportunities\n");
    output.push_str(&format!(
        "Database: {}\n",
        insights::insights_database_path()?.display(),
    ));
    output.push_str(&format!(
        "Window: {window_label} | minimum confidence: {}\n",
        minimum_confidence.as_str(),
    ));
    if opportunities.is_empty() {
        output.push_str("No potential compression opportunities recorded.");
        return Ok(ProxyOutcome::success(output));
    }
    output.push_str(&format_opportunities(&opportunities));
    Ok(ProxyOutcome::success(output))
}

fn parse_since(value: &str) -> Result<(Option<u64>, String)> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "all" {
        return Ok((None, "all recorded time".to_string()));
    }
    let (digits, unit, multiplier_ms) = if let Some(digits) = normalized.strip_suffix('h') {
        (digits, "hours", 60_u64 * 60 * 1_000)
    } else if let Some(digits) = normalized.strip_suffix('d') {
        (digits, "days", 24_u64 * 60 * 60 * 1_000)
    } else {
        bail!("invalid --since value '{value}'; use <N>h, <N>d, or all")
    };
    let amount = digits
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid --since value '{value}'; use <N>h, <N>d, or all"))?;
    if amount == 0 {
        bail!("invalid --since value '{value}'; duration must be greater than zero")
    }
    let window_ms = amount
        .checked_mul(multiplier_ms)
        .ok_or_else(|| anyhow::anyhow!("--since duration '{value}' is too large"))?;
    let now_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    Ok((
        Some(now_ms.saturating_sub(window_ms)),
        format!("last {amount} {unit}"),
    ))
}

pub(super) fn command_opportunity_json(
    opportunity: &CommandOpportunityInsight,
) -> serde_json::Value {
    serde_json::json!({
        "process": opportunity.process,
        "command_family": opportunity.command_family,
        "samples": opportunity.samples,
        "latest_at_ms": opportunity.latest_at_ms,
        "raw": metrics_json(&opportunity.raw),
        "projected": metrics_json(&opportunity.projected),
        "potential_saved": metrics_json(&opportunity.potential_saved),
        "avg_potential_saved_tokens": div_floor(
            opportunity.potential_saved.tokens,
            opportunity.samples
        ),
        "best_potential_saved_tokens": opportunity.best_potential_saved_tokens,
        "strategy": opportunity.strategy,
        "confidence": opportunity.confidence.as_str(),
    })
}

pub(super) fn push_opportunity_csv_rows(
    output: &mut String,
    opportunities: &[CommandOpportunityInsight],
) {
    for (index, opportunity) in opportunities.iter().enumerate() {
        let base = CsvMetricRow::new("passthrough_opportunities", "", "")
            .rank(index + 1)
            .process(&opportunity.process)
            .command_family(&opportunity.command_family)
            .command(&opportunity.command_family);
        push_metric_row(output, base.clone().metric("samples", opportunity.samples));
        push_metric_row(
            output,
            base.clone().metric(
                "best_potential_saved_tokens",
                opportunity.best_potential_saved_tokens,
            ),
        );
        push_metric_row(
            output,
            base.clone().metric("strategy", &opportunity.strategy),
        );
        push_metric_row(
            output,
            base.clone()
                .metric("confidence", opportunity.confidence.as_str()),
        );
        push_metric_group(
            output,
            "passthrough_opportunities",
            "potential_saved",
            &opportunity.potential_saved,
            |section, metric, value| base.clone().section(section).metric(metric, value),
        );
        push_metric_group(
            output,
            "passthrough_opportunities",
            "raw",
            &opportunity.raw,
            |section, metric, value| base.clone().section(section).metric(metric, value),
        );
        push_metric_group(
            output,
            "passthrough_opportunities",
            "projected",
            &opportunity.projected,
            |section, metric, value| base.clone().section(section).metric(metric, value),
        );
    }
}

fn format_opportunities(opportunities: &[CommandOpportunityInsight]) -> String {
    if opportunities.is_empty() {
        return "(no passthrough opportunities)\n".to_string();
    }
    let mut output = String::from(
        "process | family | confidence | samples | potential saved tokens | potential saved lines | potential saved chars | avg potential tokens | best potential tokens | strategy\n",
    );
    for opportunity in opportunities {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            opportunity.process,
            opportunity.command_family,
            opportunity.confidence.as_str(),
            format_count(opportunity.samples),
            format_count(opportunity.potential_saved.tokens),
            format_count(opportunity.potential_saved.lines),
            format_count(opportunity.potential_saved.chars),
            format_count(div_floor(
                opportunity.potential_saved.tokens,
                opportunity.samples
            )),
            format_count(opportunity.best_potential_saved_tokens),
            opportunity.strategy,
        ));
    }
    output.trim_end().to_string()
}
