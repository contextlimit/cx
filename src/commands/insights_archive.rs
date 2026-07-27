use std::path::PathBuf;

use anyhow::Result;

use crate::support::{insights, runner::ProxyOutcome};

pub fn run_archive_summary(archives: &[PathBuf], limit: usize) -> Result<ProxyOutcome> {
    let summary = insights::archive_summary(archives, limit)?;
    let mut output = String::from("cx insights: archive summary\n");
    output.push_str("Dedupe basis: canonical source-command rows\n\n");
    output.push_str("Archives:\n");
    output.push_str(&format_archive_paths(&summary.archives));
    output.push_str("\n\nDedupe:\n");
    output.push_str(&format_archive_dedupe(&summary.dedupe));
    output.push_str("\n\nCanonical totals:\n");
    output.push_str(&format_archive_overall(&summary.overall));
    output.push_str("\n\nOfficial vs passthrough:\n");
    output.push_str(&format_archive_support_classes(
        &summary.support_classes,
        summary.overall.raw.tokens,
    ));
    output.push_str("\n\nTop saved command families:\n");
    output.push_str(&format_archive_command_totals(&summary.top_saved_commands));
    output.push_str("\n\nTop zero-savings opportunities:\n");
    output.push_str(&format_archive_command_totals(
        &summary.top_opportunity_commands,
    ));
    output.push_str("\n\nSource machines:\n");
    output.push_str(&format_archive_source_totals(&summary.source_machines));
    output.push_str("\n\nGit roots:\n");
    output.push_str(&format_archive_source_totals(&summary.git_roots));
    output.push_str("\n\nCommand-shape quality:\n");
    output.push_str(&format_archive_command_quality(&summary.command_quality));
    output.push_str("\n\nExpansion rows:\n");
    output.push_str(&format_archive_command_totals(&summary.expansion));
    output.push_str("\n\nFailure/report coverage:\n");
    output.push_str(&format_archive_coverage(
        &summary.failure_coverage,
        &summary.report_coverage,
    ));
    Ok(ProxyOutcome::success(output))
}

fn format_archive_paths(paths: &[insights::ArchivePathInsight]) -> String {
    if paths.is_empty() {
        return "(no archives)\n".to_string();
    }
    let mut output = String::from("archive | rows | distinct invocations | first utc | last utc\n");
    for path in paths {
        output.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            path.path.display(),
            format_count(path.rows),
            format_count(path.distinct_invocations),
            path.first_utc,
            path.last_utc,
        ));
    }
    output.trim_end().to_string()
}

fn format_archive_dedupe(dedupe: &insights::ArchiveDedupeInsight) -> String {
    format!(
        "Raw archive rows: {}\nInvocation-id rows: {}\nCanonical source-command rows: {}\nArchive duplicate rows: {}\nImport fanout rows: {}",
        format_count(dedupe.raw_archive_rows),
        format_count(dedupe.invocation_id_rows),
        format_count(dedupe.canonical_rows),
        format_count(dedupe.archive_duplicate_rows),
        format_count(dedupe.import_fanout_rows),
    )
}

fn format_archive_overall(overall: &insights::ArchiveOverallInsight) -> String {
    format!(
        "Invocations: {} ({} nonzero exits)\nRaw: {} bytes, {} estimated tokens, {} lines\nEmitted: {} bytes, {} estimated tokens, {} lines\nSaved: {} bytes, {} estimated tokens, {} lines\nSavings: {} tokens, {} bytes, {} lines\nEmpty argv rows: {} ({})",
        format_count(overall.invocations),
        format_count(overall.nonzero),
        format_count(overall.raw.bytes),
        format_count(overall.raw.tokens),
        format_count(overall.raw.lines),
        format_count(overall.emitted.bytes),
        format_count(overall.emitted.tokens),
        format_count(overall.emitted.lines),
        format_count(overall.saved.bytes),
        format_count(overall.saved.tokens),
        format_count(overall.saved.lines),
        format_ratio(overall.saved.tokens, overall.raw.tokens),
        format_ratio(overall.saved.bytes, overall.raw.bytes),
        format_ratio(overall.saved.lines, overall.raw.lines),
        format_count(overall.empty_argv),
        format_ratio(overall.empty_argv, overall.invocations),
    )
}

fn format_archive_support_classes(
    rows: &[insights::ArchiveSupportClassInsight],
    total_raw_tokens: u64,
) -> String {
    if rows.is_empty() {
        return "(no support-class rows)\n".to_string();
    }
    let mut output = String::from(
        "class | invocations | raw tokens | raw token share | saved tokens | savings | nonzero\n",
    );
    for row in rows {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {}\n",
            row.support_class,
            format_count(row.invocations),
            format_count(row.raw_tokens),
            format_ratio(row.raw_tokens, total_raw_tokens),
            format_count(row.saved_tokens),
            format_ratio(row.saved_tokens, row.raw_tokens),
            format_count(row.nonzero),
        ));
    }
    output.trim_end().to_string()
}

fn format_archive_command_totals(rows: &[insights::ArchiveCommandTotalInsight]) -> String {
    if rows.is_empty() {
        return "(no command rows)\n".to_string();
    }
    let mut output = String::from(
        "family | process | invocations | nonzero | raw tokens | emitted tokens | saved tokens | savings\n",
    );
    for row in rows {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {}\n",
            row.command_family,
            row.process,
            format_count(row.invocations),
            format_count(row.nonzero),
            format_count(row.raw_tokens),
            format_count(row.emitted_tokens),
            format_count(row.saved_tokens),
            format_ratio(row.saved_tokens, row.raw_tokens),
        ));
    }
    output.trim_end().to_string()
}

fn format_archive_source_totals(rows: &[insights::ArchiveSourceTotalInsight]) -> String {
    if rows.is_empty() {
        return "(no source rows)\n".to_string();
    }
    let mut output =
        String::from("name | invocations | nonzero | raw tokens | saved tokens | savings\n");
    for row in rows {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {}\n",
            row.name,
            format_count(row.invocations),
            format_count(row.nonzero),
            format_count(row.raw_tokens),
            format_count(row.saved_tokens),
            format_ratio(row.saved_tokens, row.raw_tokens),
        ));
    }
    output.trim_end().to_string()
}

fn format_archive_command_quality(rows: &[insights::ArchiveCommandQualityInsight]) -> String {
    if rows.is_empty() {
        return "(no command-quality rows)\n".to_string();
    }
    let mut output = String::from(
        "family | invocations | empty argv | empty argv % | family-only command | family-only % | empty shape | empty shape % | family-only shape | family-only shape %\n",
    );
    for row in rows {
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            row.command_family,
            format_count(row.invocations),
            format_count(row.empty_argv),
            format_ratio(row.empty_argv, row.invocations),
            format_count(row.family_only_command),
            format_ratio(row.family_only_command, row.invocations),
            format_count(row.empty_shape),
            format_ratio(row.empty_shape, row.invocations),
            format_count(row.family_only_shape),
            format_ratio(row.family_only_shape, row.invocations),
        ));
    }
    output.trim_end().to_string()
}

fn format_archive_coverage(
    failures: &insights::ArchiveCoverageInsight,
    reports: &insights::ArchiveCoverageInsight,
) -> String {
    format!(
        "Failures: {} rows, {} distinct, {} with artifacts, {} with previews\nReports: {} rows, {} distinct, {} with text",
        format_count(failures.rows),
        format_count(failures.distinct_rows),
        format_count(failures.artifact_rows),
        format_count(failures.preview_rows),
        format_count(reports.rows),
        format_count(reports.distinct_rows),
        format_count(reports.preview_rows),
    )
}

fn format_ratio(part: u64, whole: u64) -> String {
    format!("{:.2}%", ratio_value(part, whole) * 100.0)
}

fn ratio_value(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn format_count(value: u64) -> String {
    let text = value.to_string();
    let mut output = String::with_capacity(text.len() + text.len() / 3);
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    output.chars().rev().collect()
}
