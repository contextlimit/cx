use crate::support::insights::SavingsDistributionInsight;

use super::csv_view::{push_metric_row, CsvMetricRow};
use super::format_utils::{format_count, format_ratio};

pub(super) fn format_savings_distribution(distribution: &SavingsDistributionInsight) -> String {
    format!(
        "Saving invocations: {} of {} ({})\nAll-invocation saved-token percentiles: p50 {}, p95 {}, p99 {}\nSaving-invocation saved-token percentiles: p50 {}, p95 {}, p99 {}\nLargest invocation: {} tokens ({})\nTop 10 invocations: {} tokens ({})\nSaved excluding largest invocation: {} tokens\nSaved excluding top 10 invocations: {} tokens",
        format_count(distribution.saving_invocations),
        format_count(distribution.invocations),
        format_ratio(distribution.saving_invocations, distribution.invocations),
        format_count(distribution.all_p50_saved_tokens),
        format_count(distribution.all_p95_saved_tokens),
        format_count(distribution.all_p99_saved_tokens),
        format_count(distribution.saving_p50_saved_tokens),
        format_count(distribution.saving_p95_saved_tokens),
        format_count(distribution.saving_p99_saved_tokens),
        format_count(distribution.largest_saved_tokens),
        format_ratio(
            distribution.largest_saved_tokens,
            distribution.total_saved_tokens,
        ),
        format_count(distribution.top_ten_saved_tokens),
        format_ratio(
            distribution.top_ten_saved_tokens,
            distribution.total_saved_tokens,
        ),
        format_count(distribution.saved_tokens_excluding_largest()),
        format_count(distribution.saved_tokens_excluding_top_ten()),
    )
}

pub(super) fn savings_distribution_json(
    distribution: &SavingsDistributionInsight,
) -> serde_json::Value {
    serde_json::json!({
        "invocations": distribution.invocations,
        "saving_invocations": distribution.saving_invocations,
        "saving_invocation_rate": distribution.saving_invocation_rate(),
        "percentiles": {
            "all_invocations": {
                "p50_saved_tokens": distribution.all_p50_saved_tokens,
                "p95_saved_tokens": distribution.all_p95_saved_tokens,
                "p99_saved_tokens": distribution.all_p99_saved_tokens,
            },
            "saving_invocations": {
                "p50_saved_tokens": distribution.saving_p50_saved_tokens,
                "p95_saved_tokens": distribution.saving_p95_saved_tokens,
                "p99_saved_tokens": distribution.saving_p99_saved_tokens,
            },
        },
        "concentration": {
            "total_saved_tokens": distribution.total_saved_tokens,
            "largest_saved_tokens": distribution.largest_saved_tokens,
            "largest_share": distribution.largest_share(),
            "top_10_saved_tokens": distribution.top_ten_saved_tokens,
            "top_10_share": distribution.top_ten_share(),
            "saved_tokens_excluding_largest": distribution.saved_tokens_excluding_largest(),
            "saved_tokens_excluding_top_10": distribution.saved_tokens_excluding_top_ten(),
        },
    })
}

pub(super) fn push_savings_distribution_csv_rows(
    output: &mut String,
    distribution: &SavingsDistributionInsight,
) {
    for (metric, value) in [
        ("invocations", distribution.invocations),
        ("saving_invocations", distribution.saving_invocations),
        ("all_p50_saved_tokens", distribution.all_p50_saved_tokens),
        ("all_p95_saved_tokens", distribution.all_p95_saved_tokens),
        ("all_p99_saved_tokens", distribution.all_p99_saved_tokens),
        (
            "saving_p50_saved_tokens",
            distribution.saving_p50_saved_tokens,
        ),
        (
            "saving_p95_saved_tokens",
            distribution.saving_p95_saved_tokens,
        ),
        (
            "saving_p99_saved_tokens",
            distribution.saving_p99_saved_tokens,
        ),
        ("total_saved_tokens", distribution.total_saved_tokens),
        ("largest_saved_tokens", distribution.largest_saved_tokens),
        ("top_10_saved_tokens", distribution.top_ten_saved_tokens),
        (
            "saved_tokens_excluding_largest",
            distribution.saved_tokens_excluding_largest(),
        ),
        (
            "saved_tokens_excluding_top_10",
            distribution.saved_tokens_excluding_top_ten(),
        ),
    ] {
        push_metric_row(
            output,
            CsvMetricRow::new("savings_distribution", metric, value),
        );
    }
    for (metric, value) in [
        (
            "saving_invocation_rate",
            distribution.saving_invocation_rate(),
        ),
        ("largest_share", distribution.largest_share()),
        ("top_10_share", distribution.top_ten_share()),
    ] {
        push_metric_row(
            output,
            CsvMetricRow::new("savings_distribution", metric, format!("{value:.6}")),
        );
    }
}
