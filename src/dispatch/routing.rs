use anyhow::{Context, Result};

use crate::cli::{
    CargoProxyCommand, CmakeProxyCommand, Command, DockerProxyCommand, GitProxyCommand,
    GoProxyCommand, InsightsAuditFormat, InsightsCommand, InsightsExportFormat,
    InsightsOpportunityConfidence, InsightsReportDenialReason, InsightsReportStatus,
    InsightsReportStatusFilter, InsightsReportTriageFormat, InsightsSavingsSort, InsightsTopSort,
    KubectlProxyCommand,
};
use crate::commands;
use crate::support::insights;
use crate::support::runner::ProxyOutcome;

use super::read::{build_read_options, ReadDispatchArgs};

pub(super) fn execute_command(command: &Command) -> Result<ProxyOutcome> {
    match command {
        Command::Git { command } => execute_git_command(command),
        Command::Diff { args } => commands::git::run_diff(args),
        Command::Read {
            file,
            head,
            tail,
            range,
            full,
            line_numbers,
            raw,
            mode,
            smart,
            max_lines,
            no_auto_aggressive,
        } => {
            let read_args = ReadDispatchArgs {
                head: *head,
                tail: *tail,
                range_spec: range.as_deref(),
                full: *full,
                line_numbers: *line_numbers,
                raw: *raw,
                mode: *mode,
                smart: *smart,
                max_lines: *max_lines,
                no_auto_aggressive: *no_auto_aggressive,
            };
            execute_read_command(file.as_path(), read_args)
        }
        Command::Grep { .. } => execute_grep_command(command),
        Command::Ls { args } => commands::ls::run(args),
        Command::Cat { args } => commands::read_like::run_cat(args),
        Command::Head { args } => commands::read_like::run_head(args),
        Command::Tail { args } => commands::read_like::run_tail(args),
        Command::Sed { args } => commands::read_like::run_sed(args),
        Command::Nl { args } => commands::read_like::run_nl(args),
        Command::Ps { args } => commands::ps_cmd::run(args),
        Command::Pytest { args } => commands::pytest_cmd::run(args),
        Command::Cargo { command } => execute_cargo_command(command),
        Command::Go { command } => execute_go_command(command),
        Command::Tsc { args } => commands::tsc_cmd::run(args),
        Command::Node { args } => commands::node_cmd::run(args),
        Command::Sh { no_compact, args } => {
            commands::shell_cmd::run_with_options(args, *no_compact)
        }
        Command::Cmake { command } => execute_cmake_command(command),
        Command::Ctest { args } => commands::ctest_cmd::run(args),
        Command::Find { args } => commands::find::run(args),
        Command::Docker { command } => execute_docker_command(command),
        Command::Kubectl { command } => execute_kubectl_command(command),
        Command::Report { args } => commands::report::run(args),
        Command::Passthrough { args } => commands::passthrough::run(args),
        Command::Insights { command } => execute_insights_command(command),
    }
}

fn execute_git_command(command: &GitProxyCommand) -> Result<ProxyOutcome> {
    match command {
        GitProxyCommand::Status { args } => commands::git::run_status(args),
        GitProxyCommand::Diff { args } => commands::git::run_diff(args),
        GitProxyCommand::Log { args } => commands::git::run_log(args),
        GitProxyCommand::Show { args } => commands::git::run_show(args),
        GitProxyCommand::EvidenceDiff { args } => commands::git::run_evidence_diff(args),
        GitProxyCommand::ConflictDiff { args } => commands::git::run_conflict_diff(args),
    }
}

fn execute_read_command(
    file: &std::path::Path,
    args: ReadDispatchArgs<'_>,
) -> Result<ProxyOutcome> {
    let options = build_read_options(args)?;
    commands::read::run(file, &options)
}

fn execute_grep_command(command: &Command) -> Result<ProxyOutcome> {
    let Command::Grep {
        patterns,
        extended_regexp,
        line_numbers: _,
        no_heading: _,
        with_filename: _,
        recursive: _,
        ignore_case,
        smart_case,
        after_context,
        before_context,
        context,
        files_with_matches,
        hidden,
        no_ignore,
        text,
        only_matching,
        fixed_strings,
        files,
        globs,
        max_results,
        no_compact,
        terms,
    } = command
    else {
        unreachable!("execute_grep_command called with non-grep command");
    };

    let options = commands::grep::GrepOptions {
        extended_regexp: *extended_regexp,
        ignore_case: *ignore_case,
        smart_case: *smart_case,
        context_before: *before_context,
        context_after: *after_context,
        context_lines: *context,
        files_with_matches: *files_with_matches,
        hidden: *hidden,
        no_ignore: *no_ignore,
        text: *text,
        only_matching: *only_matching,
        fixed_strings: *fixed_strings,
        glob_patterns: globs.clone(),
        max_results: *max_results,
        no_compact: *no_compact,
    };
    if *files {
        commands::grep::list_files(terms, &options)
    } else {
        let (patterns, paths) = grep_patterns_and_paths(patterns, terms)?;
        commands::grep::run_many(&patterns, paths, &options)
    }
}

fn grep_patterns_and_paths<'a>(
    patterns: &'a [String],
    terms: &'a [String],
) -> Result<(Vec<String>, &'a [String])> {
    if !patterns.is_empty() {
        return Ok((patterns.to_vec(), terms));
    }
    let (pattern, paths) = terms
        .split_first()
        .context("grep requires a PATTERN unless --files is used")?;
    Ok((vec![pattern.clone()], paths))
}

fn execute_cargo_command(command: &CargoProxyCommand) -> Result<ProxyOutcome> {
    match command {
        CargoProxyCommand::Test { args } => commands::cargo_cmd::run_test(args),
    }
}

fn execute_go_command(command: &GoProxyCommand) -> Result<ProxyOutcome> {
    match command {
        GoProxyCommand::Test { args } => commands::go_cmd::run_test(args),
    }
}

fn execute_cmake_command(command: &CmakeProxyCommand) -> Result<ProxyOutcome> {
    match command {
        CmakeProxyCommand::Build { args } => commands::cmake_cmd::run_build(args),
    }
}

fn execute_docker_command(command: &DockerProxyCommand) -> Result<ProxyOutcome> {
    match command {
        DockerProxyCommand::Ps { args } => commands::container::run_docker_ps(args),
        DockerProxyCommand::Logs { container, args } => {
            commands::container::run_docker_logs(container, args)
        }
    }
}

fn execute_kubectl_command(command: &KubectlProxyCommand) -> Result<ProxyOutcome> {
    match command {
        KubectlProxyCommand::Logs { pod, args } => commands::container::run_kubectl_logs(pod, args),
    }
}

fn execute_insights_command(command: &InsightsCommand) -> Result<ProxyOutcome> {
    if matches!(
        command,
        InsightsCommand::Report { .. }
            | InsightsCommand::Reports { .. }
            | InsightsCommand::ReportUpdate { .. }
            | InsightsCommand::ReportTriage { .. }
    ) {
        execute_insights_report_command(command)
    } else {
        execute_non_report_insights_command(command)
    }
}

fn execute_insights_report_command(command: &InsightsCommand) -> Result<ProxyOutcome> {
    match command {
        InsightsCommand::Report {
            root,
            command_filter,
            limit,
        } => commands::insights::run_report(*limit, command_filter_args(root, command_filter)),
        InsightsCommand::Reports {
            level,
            status,
            root,
            command_filter,
            limit,
        } => commands::insights::run_reports(
            *limit,
            command_level(*level),
            command_filter_args(root, command_filter),
            report_status_filter(*status),
        ),
        InsightsCommand::ReportUpdate {
            report_id,
            status,
            reason,
            related_report_id,
            note,
            revision,
        } => commands::insights::run_report_update(
            *report_id,
            report_status(*status),
            reason.map(report_denial_reason),
            *related_report_id,
            note,
            revision,
        ),
        InsightsCommand::ReportTriage {
            apply,
            format,
            limit,
        } => commands::insights::run_report_triage(*apply, report_triage_format(*format), *limit),
        _ => unreachable!("non-report insights command reached report routing"),
    }
}

fn execute_non_report_insights_command(command: &InsightsCommand) -> Result<ProxyOutcome> {
    match command {
        InsightsCommand::Summary { limit } => commands::insights::run_summary(*limit),
        InsightsCommand::Top { sort, level, limit } => {
            commands::insights::run_top(command_total_sort(*sort), command_level(*level), *limit)
        }
        InsightsCommand::Largest {
            sort,
            root,
            command_filter,
            limit,
        } => commands::insights::run_largest(
            savings_sort(*sort),
            *limit,
            command_filter_args(root, command_filter),
        ),
        InsightsCommand::Recent {
            root,
            command_filter,
            limit,
        } => commands::insights::run_recent(*limit, command_filter_args(root, command_filter)),
        InsightsCommand::Daily { limit } => commands::insights::run_daily(*limit),
        InsightsCommand::Expansions {
            root,
            command_filter,
            limit,
        } => commands::insights::run_expansions(*limit, command_filter_args(root, command_filter)),
        InsightsCommand::Presentation { limit } => commands::insights::run_presentation(*limit),
        InsightsCommand::Dashboard {
            root,
            command_filter,
            limit,
        } => commands::insights::run_dashboard(*limit, command_filter_args(root, command_filter)),
        InsightsCommand::Audit {
            root,
            command_filter,
            format,
            limit,
        } => commands::insights_audit::run(
            *limit,
            command_filter_args(root, command_filter),
            audit_format(*format),
        ),
        InsightsCommand::Settings { set } => commands::insights::run_settings(set),
        InsightsCommand::Impact {
            limit,
            context_window_tokens,
        } => commands::insights::run_impact(*limit, *context_window_tokens),
        InsightsCommand::Recommend { limit } => commands::insights::run_recommend(*limit),
        InsightsCommand::Opportunities {
            limit,
            since,
            min_confidence,
        } => commands::insights::run_opportunities_filtered(
            *limit,
            since,
            opportunity_confidence(*min_confidence),
        ),
        InsightsCommand::Routing {
            root,
            command_filter,
            limit,
        } => commands::insights::run_routing(*limit, command_filter_args(root, command_filter)),
        InsightsCommand::ArchiveSummary { archives, limit } => {
            commands::insights::run_archive_summary(archives, *limit)
        }
        InsightsCommand::Failures {
            level,
            root,
            command_filter,
            limit,
        } => commands::insights::run_failures(
            *limit,
            command_level(*level),
            command_filter_args(root, command_filter),
        ),
        InsightsCommand::Export {
            format,
            root,
            command_filter,
            limit,
        } => commands::insights::run_export(
            export_format(*format),
            *limit,
            command_filter_args(root, command_filter),
        ),
        InsightsCommand::Report { .. }
        | InsightsCommand::Reports { .. }
        | InsightsCommand::ReportUpdate { .. }
        | InsightsCommand::ReportTriage { .. } => {
            unreachable!("report insights command reached non-report routing")
        }
    }
}

const fn opportunity_confidence(
    confidence: InsightsOpportunityConfidence,
) -> insights::OpportunityConfidence {
    match confidence {
        InsightsOpportunityConfidence::Low => insights::OpportunityConfidence::Low,
        InsightsOpportunityConfidence::Medium => insights::OpportunityConfidence::Medium,
        InsightsOpportunityConfidence::High => insights::OpportunityConfidence::High,
    }
}

fn command_total_sort(sort: InsightsTopSort) -> insights::CommandTotalSort {
    match sort {
        InsightsTopSort::Tokens => insights::CommandTotalSort::Tokens,
        InsightsTopSort::Chars => insights::CommandTotalSort::Chars,
        InsightsTopSort::Lines => insights::CommandTotalSort::Lines,
        InsightsTopSort::Invocations => insights::CommandTotalSort::Invocations,
        InsightsTopSort::Failures => insights::CommandTotalSort::Failures,
    }
}

fn command_level(level: crate::cli::InsightsCommandLevel) -> insights::CommandLevel {
    match level {
        crate::cli::InsightsCommandLevel::Command => insights::CommandLevel::Command,
        crate::cli::InsightsCommandLevel::Root => insights::CommandLevel::Root,
    }
}

fn command_filter_args<'a>(
    root: &'a Option<String>,
    command_filter: &'a Option<String>,
) -> insights::CommandFilter<'a> {
    insights::CommandFilter {
        command_root: root.as_deref(),
        command: command_filter.as_deref(),
    }
}

fn savings_sort(sort: InsightsSavingsSort) -> insights::SavingsSort {
    match sort {
        InsightsSavingsSort::Tokens => insights::SavingsSort::Tokens,
        InsightsSavingsSort::Chars => insights::SavingsSort::Chars,
        InsightsSavingsSort::Lines => insights::SavingsSort::Lines,
    }
}

fn report_status_filter(
    status: InsightsReportStatusFilter,
) -> Option<insights::CommandReportStatus> {
    match status {
        InsightsReportStatusFilter::All => None,
        InsightsReportStatusFilter::Open => Some(insights::CommandReportStatus::Open),
        InsightsReportStatusFilter::Resolved => Some(insights::CommandReportStatus::Resolved),
        InsightsReportStatusFilter::NativeParity => {
            Some(insights::CommandReportStatus::NativeParity)
        }
        InsightsReportStatusFilter::NotReproducible => {
            Some(insights::CommandReportStatus::NotReproducible)
        }
        InsightsReportStatusFilter::Denied => Some(insights::CommandReportStatus::Denied),
    }
}

fn report_status(status: InsightsReportStatus) -> insights::CommandReportStatus {
    match status {
        InsightsReportStatus::Open => insights::CommandReportStatus::Open,
        InsightsReportStatus::Resolved => insights::CommandReportStatus::Resolved,
        InsightsReportStatus::NativeParity => insights::CommandReportStatus::NativeParity,
        InsightsReportStatus::NotReproducible => insights::CommandReportStatus::NotReproducible,
        InsightsReportStatus::Denied => insights::CommandReportStatus::Denied,
    }
}

fn report_denial_reason(reason: InsightsReportDenialReason) -> insights::CommandReportDenialReason {
    match reason {
        InsightsReportDenialReason::Duplicate => insights::CommandReportDenialReason::Duplicate,
        InsightsReportDenialReason::InsufficientEvidence => {
            insights::CommandReportDenialReason::InsufficientEvidence
        }
        InsightsReportDenialReason::Invalid => insights::CommandReportDenialReason::Invalid,
        InsightsReportDenialReason::Obsolete => insights::CommandReportDenialReason::Obsolete,
        InsightsReportDenialReason::Unsupported => insights::CommandReportDenialReason::Unsupported,
        InsightsReportDenialReason::LowValue => insights::CommandReportDenialReason::LowValue,
    }
}

fn report_triage_format(
    format: InsightsReportTriageFormat,
) -> commands::insights::ReportTriageFormat {
    match format {
        InsightsReportTriageFormat::Text => commands::insights::ReportTriageFormat::Text,
        InsightsReportTriageFormat::Json => commands::insights::ReportTriageFormat::Json,
    }
}

fn export_format(format: InsightsExportFormat) -> commands::insights::ExportFormat {
    match format {
        InsightsExportFormat::Json => commands::insights::ExportFormat::Json,
        InsightsExportFormat::Csv => commands::insights::ExportFormat::Csv,
    }
}

fn audit_format(format: InsightsAuditFormat) -> commands::insights_audit::AuditFormat {
    match format {
        InsightsAuditFormat::Text => commands::insights_audit::AuditFormat::Text,
        InsightsAuditFormat::Json => commands::insights_audit::AuditFormat::Json,
    }
}
