use crate::cli::{
    CargoProxyCommand, Cli, CmakeProxyCommand, Command, DockerProxyCommand, GitProxyCommand,
    GoProxyCommand, KubectlProxyCommand,
};
use crate::commands;
use crate::commands::command_identity::CommandIdentity;
use crate::support::insights::{
    self, FailureDetailRecord, InvocationContext, InvocationRecord, TextMetrics,
};
use crate::support::redaction;
use crate::support::runner::ProxyOutcome;

use super::output::emitted_text;

pub(super) fn record_insights(cli: &Cli, outcome: &ProxyOutcome) {
    let command = &cli.command;
    let emitted_text = emitted_text(outcome);
    let emitted = TextMetrics::from_text(&emitted_text);
    let command_label = invocation_command_label(cli);
    let process = command_root(command, &command_label);
    let invocation_command = invocation_command_line(cli, command, &command_label);
    let argv_json = redacted_argv_json(&cli.raw_args);
    let record = InvocationRecord {
        command: &command_label,
        exit_code: outcome.exit_code,
        raw: outcome.observation.as_ref(),
        emitted,
    };
    let context = InvocationContext {
        process: &process,
        command: &invocation_command,
        argv_json: &argv_json,
        emitted_response: Some(&emitted_text),
    };
    let raw_source = outcome.observation.as_ref().map(|raw| raw.source.as_str());
    let raw_response = outcome
        .observation
        .as_ref()
        .and_then(|raw| raw.response.as_deref());
    let actionable_failure =
        insights::exit_code_is_actionable_failure(&command_label, outcome.exit_code);
    let command_line = actionable_failure.then(|| command_line(cli, &command_label));
    let failure_detail = command_line
        .as_ref()
        .map(|command_line| FailureDetailRecord {
            command_family: &command_label,
            command_line,
            exit_code: outcome.exit_code,
            cx_response: &emitted_text,
            raw_source,
            raw_response,
        });
    if let Err(error) = insights::record_invocation_with_context_and_failure(
        &record,
        Some(&context),
        failure_detail.as_ref(),
    ) {
        if std::env::var("CX_INSIGHTS_DEBUG").is_ok() {
            eprintln!("cx insights: {error:#}");
        }
    }
}

fn command_line(cli: &Cli, command_label: &str) -> String {
    if cli.raw_args.len() > 1 {
        redaction::redacted_shell_join(&cli.raw_args)
    } else {
        format!("cx {command_label}")
    }
}

fn invocation_command_line(cli: &Cli, command: &Command, command_label: &str) -> String {
    if let Command::Passthrough { args } = command {
        return redaction::redacted_shell_join(args);
    }
    let args = invocation_args(&cli.raw_args);
    if !args.is_empty() {
        return redaction::redacted_shell_join(args);
    }
    command_label.to_string()
}

fn invocation_args(args: &[String]) -> &[String] {
    let mut start = usize::from(args.first().is_some_and(|arg| arg == "cx"));
    if args.get(start).is_some_and(|arg| arg == "--") {
        start += 1;
    }
    &args[start..]
}

fn redacted_argv_json(args: &[String]) -> String {
    serde_json::to_string(&redaction::redact_argv(args)).unwrap_or_else(|_| "[]".to_string())
}

pub(super) fn command_label(command: &Command) -> String {
    match command {
        Command::Git { command } => match command {
            GitProxyCommand::Status { .. } => "git status".to_string(),
            GitProxyCommand::Diff { .. } => "git diff".to_string(),
            GitProxyCommand::Log { .. } => "git log".to_string(),
            GitProxyCommand::Show { .. } => "git show".to_string(),
            GitProxyCommand::EvidenceDiff { .. } => "git evidence-diff".to_string(),
            GitProxyCommand::ConflictDiff { .. } => "git conflict-diff".to_string(),
        },
        Command::Diff { .. } => "diff".to_string(),
        Command::Read { .. } => "read".to_string(),
        Command::Grep { files, .. } if *files => "grep files".to_string(),
        Command::Grep { .. } => "grep".to_string(),
        Command::Ls { .. } => "ls".to_string(),
        Command::Cat { .. } => "cat".to_string(),
        Command::Head { .. } => "head".to_string(),
        Command::Tail { .. } => "tail".to_string(),
        Command::Sed { .. } => "sed range".to_string(),
        Command::Nl { .. } => "nl".to_string(),
        Command::Ps { .. } => "ps".to_string(),
        Command::Pytest { .. } => "pytest".to_string(),
        Command::Cargo { command } => match command {
            CargoProxyCommand::Test { .. } => "cargo test".to_string(),
        },
        Command::Go { command } => match command {
            GoProxyCommand::Test { .. } => "go test".to_string(),
        },
        Command::Tsc { .. } => "tsc".to_string(),
        Command::Node { args } => commands::node_cmd::command_label(args).to_string(),
        Command::Sh { .. } => "sh".to_string(),
        Command::Cmake { command } => match command {
            CmakeProxyCommand::Build { .. } => "cmake build".to_string(),
        },
        Command::Ctest { .. } => "ctest".to_string(),
        Command::Find { .. } => "find".to_string(),
        Command::Docker { command } => match command {
            DockerProxyCommand::Ps { .. } => "docker ps".to_string(),
            DockerProxyCommand::Logs { .. } => "docker logs".to_string(),
        },
        Command::Kubectl { command } => match command {
            KubectlProxyCommand::Logs { .. } => "kubectl logs".to_string(),
        },
        Command::Report { .. } => "report".to_string(),
        Command::Passthrough { args } => CommandIdentity::classify(args).family,
        Command::Insights { .. } => "insights".to_string(),
    }
}

pub(super) fn invocation_command_label(cli: &Cli) -> String {
    if let Command::Grep {
        extended_regexp,
        fixed_strings,
        files,
        ..
    } = &cli.command
    {
        return commands::grep::reporting::command_family(
            &cli.raw_args,
            *extended_regexp,
            *fixed_strings,
            *files,
        );
    }
    command_label(&cli.command)
}

pub(super) fn command_root(command: &Command, command_label: &str) -> String {
    match command {
        Command::Passthrough { args } => CommandIdentity::classify(args).root,
        _ => insights::command_root(command_label).to_string(),
    }
}
