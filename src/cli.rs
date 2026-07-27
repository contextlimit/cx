use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[cfg(test)]
use clap::CommandFactory;

#[derive(Debug, Parser)]
#[command(
    name = "cx",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("CX_BUILD_REVISION"), ")")
)]
pub struct Cli {
    #[arg(skip)]
    pub raw_args: Vec<String>,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn parse_cx() -> Self {
        parse_from_cx_args(std::env::args_os())
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Git {
        #[command(subcommand)]
        command: GitProxyCommand,
    },
    Diff {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Read {
        file: PathBuf,
        #[arg(long, conflicts_with_all = ["tail", "full"])]
        head: Option<usize>,
        #[arg(long, conflicts_with_all = ["head", "full"])]
        tail: Option<usize>,
        #[arg(long, value_name = "START:END", conflicts_with_all = ["head", "tail", "full"])]
        range: Option<String>,
        #[arg(long, default_value_t = false)]
        full: bool,
        #[arg(long, short = 'n', default_value_t = false)]
        line_numbers: bool,
        #[arg(long, default_value_t = false)]
        raw: bool,
        #[arg(long, value_enum)]
        mode: Option<ReadCliMode>,
        #[arg(long, default_value_t = false, conflicts_with = "mode")]
        smart: bool,
        #[arg(long)]
        max_lines: Option<usize>,
        #[arg(long, default_value_t = false)]
        no_auto_aggressive: bool,
    },
    #[command(visible_alias = "rg")]
    Grep {
        #[arg(
            short = 'e',
            long = "regexp",
            value_name = "PATTERN",
            allow_hyphen_values = true
        )]
        patterns: Vec<String>,
        #[arg(short = 'E', long = "extended-regexp", default_value_t = false)]
        extended_regexp: bool,
        #[arg(short = 'n', long = "line-number", default_value_t = false)]
        line_numbers: bool,
        #[arg(long = "no-heading", default_value_t = false)]
        no_heading: bool,
        #[arg(short = 'H', long = "with-filename", default_value_t = false)]
        with_filename: bool,
        #[arg(short = 'R', long = "recursive", default_value_t = false)]
        recursive: bool,
        #[arg(short = 'i', long = "ignore-case", default_value_t = false)]
        ignore_case: bool,
        #[arg(short = 'S', long = "smart-case", default_value_t = false)]
        smart_case: bool,
        #[arg(short = 'A', long = "after-context")]
        after_context: Option<usize>,
        #[arg(short = 'B', long = "before-context")]
        before_context: Option<usize>,
        #[arg(short = 'C', long = "context")]
        context: Option<usize>,
        #[arg(short = 'l', long = "files-with-matches", default_value_t = false)]
        files_with_matches: bool,
        #[arg(long = "hidden", default_value_t = false)]
        hidden: bool,
        #[arg(long = "no-ignore", default_value_t = false)]
        no_ignore: bool,
        #[arg(short = 'a', long = "text", default_value_t = false)]
        text: bool,
        #[arg(short = 'o', long = "only-matching", default_value_t = false)]
        only_matching: bool,
        #[arg(long = "fixed-strings", short = 'F', default_value_t = false)]
        fixed_strings: bool,
        #[arg(long = "files", default_value_t = false)]
        files: bool,
        #[arg(long = "glob", short = 'g')]
        globs: Vec<String>,
        #[arg(long = "max-results", alias = "head")]
        max_results: Option<usize>,
        #[arg(long, default_value_t = false)]
        no_compact: bool,
        #[arg(num_args = 0..)]
        terms: Vec<String>,
    },
    Ls {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Cat {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Head {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Tail {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Sed {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Nl {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Ps {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Pytest {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Cargo {
        #[command(subcommand)]
        command: CargoProxyCommand,
    },
    Go {
        #[command(subcommand)]
        command: GoProxyCommand,
    },
    Tsc {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Node {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Sh {
        #[arg(long, help = "Preserve shell output without CX compaction")]
        no_compact: bool,
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Cmake {
        #[command(subcommand)]
        command: CmakeProxyCommand,
    },
    Ctest {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Find {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Docker {
        #[command(subcommand)]
        command: DockerProxyCommand,
    },
    Kubectl {
        #[command(subcommand)]
        command: KubectlProxyCommand,
    },
    Report {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 1..
        )]
        args: Vec<String>,
    },
    #[command(name = "__cx_internal_passthrough", hide = true)]
    Passthrough {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 1..
        )]
        args: Vec<String>,
    },
    Insights {
        #[command(subcommand)]
        command: InsightsCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum GitProxyCommand {
    Status {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Diff {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Log {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Show {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    EvidenceDiff {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    ConflictDiff {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReadCliMode {
    Normal,
    Aggressive,
    Smart,
}

#[derive(Debug, Subcommand)]
pub enum CargoProxyCommand {
    Test {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum GoProxyCommand {
    Test {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum CmakeProxyCommand {
    Build {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum DockerProxyCommand {
    Ps {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
    Logs {
        container: String,
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum KubectlProxyCommand {
    Logs {
        pod: String,
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum InsightsCommand {
    Summary {
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    Top {
        #[arg(long, value_enum, default_value_t = InsightsTopSort::Tokens)]
        sort: InsightsTopSort,
        #[arg(long, value_enum, default_value_t = InsightsCommandLevel::Command)]
        level: InsightsCommandLevel,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Largest {
        #[arg(long, value_enum, default_value_t = InsightsSavingsSort::Tokens)]
        sort: InsightsSavingsSort,
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Recent {
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Daily {
        #[arg(long, default_value_t = 14)]
        limit: usize,
    },
    Expansions {
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Presentation {
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    Report {
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    Reports {
        #[arg(long, value_enum, default_value_t = InsightsCommandLevel::Command)]
        level: InsightsCommandLevel,
        #[arg(long, value_enum, default_value_t = InsightsReportStatusFilter::All)]
        status: InsightsReportStatusFilter,
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    ReportUpdate {
        report_id: u64,
        #[arg(long, value_enum)]
        status: InsightsReportStatus,
        #[arg(long, value_enum)]
        reason: Option<InsightsReportDenialReason>,
        #[arg(long)]
        related_report_id: Option<u64>,
        #[arg(long)]
        note: String,
        #[arg(long, default_value = "")]
        revision: String,
    },
    ReportTriage {
        #[arg(long)]
        apply: bool,
        #[arg(long, value_enum, default_value_t = InsightsReportTriageFormat::Text)]
        format: InsightsReportTriageFormat,
        #[arg(long, default_value_t = 25)]
        limit: usize,
    },
    Dashboard {
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Audit {
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, value_enum, default_value_t = InsightsAuditFormat::Text)]
        format: InsightsAuditFormat,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Settings {
        #[arg(long = "set")]
        set: Vec<String>,
    },
    Impact {
        #[arg(long, default_value_t = 5)]
        limit: usize,
        #[arg(long, default_value_t = 200_000)]
        context_window_tokens: u64,
    },
    Recommend {
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    Opportunities {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value = "7d")]
        since: String,
        #[arg(long, value_enum, default_value_t = InsightsOpportunityConfidence::Low)]
        min_confidence: InsightsOpportunityConfidence,
    },
    Routing {
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    ArchiveSummary {
        #[arg(long = "archive", value_name = "SQLITE", required = true)]
        archives: Vec<PathBuf>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Failures {
        #[arg(long, value_enum, default_value_t = InsightsCommandLevel::Command)]
        level: InsightsCommandLevel,
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Export {
        #[arg(long, value_enum, default_value_t = InsightsExportFormat::Json)]
        format: InsightsExportFormat,
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "command")]
        command_filter: Option<String>,
        #[arg(long, default_value_t = 25)]
        limit: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsTopSort {
    Tokens,
    Chars,
    Lines,
    Invocations,
    Failures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsCommandLevel {
    Command,
    Root,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsSavingsSort {
    Tokens,
    Chars,
    Lines,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsReportStatusFilter {
    All,
    Open,
    Resolved,
    NativeParity,
    NotReproducible,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsReportStatus {
    Open,
    Resolved,
    NativeParity,
    NotReproducible,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsReportDenialReason {
    Duplicate,
    InsufficientEvidence,
    Invalid,
    Obsolete,
    Unsupported,
    LowValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsReportTriageFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsAuditFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InsightsOpportunityConfidence {
    Low,
    Medium,
    High,
}

mod parse;

pub use parse::{parse_from_cx_args, try_parse_from_cx_args};

#[cfg(test)]
mod tests;
