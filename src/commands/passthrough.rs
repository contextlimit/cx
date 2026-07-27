use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::commands::command_identity::CommandIdentity;
use crate::commands::ssh_heredoc::{self, SshHeredocAction};
use crate::support::command_output;
use crate::support::insights::{
    self, CommandOpportunityRecord, OpportunityConfidence, TextMetrics,
};
use crate::support::output_projection::GENERATED_LINE_PREVIEW_CHARS;
use crate::support::output_window::{OutputWindow, ProjectedOutput};
use crate::support::redaction;
use crate::support::runner::{
    append_failure_hint, capture_with_piped_stdin, capture_with_stdin_timeout, CommandOutput,
    ProxyOutcome,
};
use crate::support::utils::resolved_command;
use crate::support::{command_repair, shell_hints};

const OPPORTUNITY_WINDOW: OutputWindow = OutputWindow::new(12, 28);
const LINE_WINDOW_STRATEGY: &str = "generic-head-tail-12-28";
const GENERATED_LINE_STRATEGY: &str = "generic-generated-line-1200";
const COMBINED_STRATEGY: &str = "generic-head-tail-12-28-generated-line-1200";
const FAILURE_ARTIFACT_TOOL: &str = "passthrough";

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    if !insights::unsupported_passthrough_enabled()? {
        bail!(
            "unsupported command passthrough is disabled; enable with `cx insights settings --set passthrough_unsupported_commands=true`"
        );
    }
    let (program, forwarded_args) = args
        .split_first()
        .context("unsupported command passthrough requires a command")?;
    if program == "cx" {
        bail!("unsupported command passthrough refuses to recursively invoke `cx`");
    }

    if program == "ssh" {
        match ssh_heredoc::inspect_ssh_args(forwarded_args) {
            SshHeredocAction::Rewrite {
                forwarded_args,
                stdin,
            } => {
                let mut cmd = resolved_command(program);
                cmd.args(&forwarded_args);
                let output =
                    capture_with_stdin_timeout(cmd, program, stdin, Duration::from_secs(600))?;
                return Ok(outcome_from_output(args, program, output));
            }
            SshHeredocAction::Reject { message } => {
                return Ok(ProxyOutcome {
                    stdout: String::new(),
                    stderr: message,
                    exit_code: 2,
                    observation: None,
                });
            }
            SshHeredocAction::None => {}
        }
    }

    let rewrite = if insights::command_optimizations_enabled().unwrap_or(true) {
        insights::insights_database_path()
            .ok()
            .and_then(|database| {
                command_repair::direct_passthrough_rewrite(program, forwarded_args, &database)
            })
    } else {
        None
    };
    let execution_args = rewrite
        .as_ref()
        .map_or(forwarded_args, |rewrite| rewrite.args.as_slice());
    let mut cmd = resolved_command(program);
    cmd.args(execution_args);
    let output = capture_passthrough(cmd, program)?;
    if let Some(rewrite) = rewrite.as_ref() {
        record_preflight_rewrite(args, rewrite, &output);
    }
    if let Some(outcome) = maybe_apply_direct_repair(args, program, execution_args, &output)? {
        return Ok(outcome);
    }
    Ok(outcome_from_output(args, program, output))
}

fn maybe_apply_direct_repair(
    original_args: &[String],
    program: &str,
    forwarded_args: &[String],
    output: &CommandOutput,
) -> Result<Option<ProxyOutcome>> {
    let Some(repair) = command_repair::direct_passthrough_repair(
        program,
        forwarded_args,
        output.exit_code,
        &output.combined,
    ) else {
        return Ok(None);
    };
    let mut cmd = resolved_command(program);
    cmd.args(&repair.args);
    let repaired_output = capture_passthrough(cmd, program)?;
    if repaired_output.exit_code != 0 {
        record_repair_attempt(
            original_args,
            &repair,
            "auto_retry_failed",
            output,
            &repaired_output,
        );
        return Ok(None);
    }
    record_repair_attempt(
        original_args,
        &repair,
        "auto_retry_success",
        output,
        &repaired_output,
    );
    let mut outcome = outcome_from_output(original_args, program, repaired_output);
    command_repair::append_note(&mut outcome.stderr, &repair.note);
    if let Some(observation) = outcome.observation.as_mut() {
        observation.use_line_terminated_streams();
    }
    Ok(Some(outcome))
}

fn outcome_from_output(args: &[String], program: &str, mut output: CommandOutput) -> ProxyOutcome {
    let failure_hint = if output.exit_code == 0 {
        None
    } else {
        output.failure_artifact_hint(FAILURE_ARTIFACT_TOOL)
    };
    let identity = CommandIdentity::classify(args);
    let mut observation = output.observation(format!("passthrough:{}", identity.root));
    let forwarded_args = args.get(1..).unwrap_or_default();
    let mut compacted = if insights::command_optimizations_enabled().unwrap_or(true) {
        command_output::classify_passthrough(program, forwarded_args).map(|profile| {
            command_output::compact_streams(
                profile,
                &output.stdout,
                &output.stderr,
                output.exit_code,
            )
        })
    } else {
        None
    };
    let was_compacted = compacted
        .as_ref()
        .is_some_and(command_output::CompactedStreams::compacted);
    if !was_compacted {
        record_opportunity(args, &output);
    }
    if !was_compacted && output.exit_code == 0 {
        observation = observation.with_preserved_stream_termination();
    }
    let stdout = compacted
        .as_mut()
        .and_then(|streams| streams.stdout.take())
        .unwrap_or_else(|| std::mem::take(&mut output.stdout));
    let mut stderr = compacted
        .as_mut()
        .and_then(|streams| streams.stderr.take())
        .unwrap_or_else(|| std::mem::take(&mut output.stderr));
    if output.exit_code != 0 {
        shell_hints::append_hint(&mut stderr, &output.combined);
    }
    ProxyOutcome {
        stdout: append_failure_hint(stdout, failure_hint.as_deref()),
        stderr,
        exit_code: output.exit_code,
        observation: None,
    }
    .with_observation(observation)
}

fn record_repair_attempt(
    args: &[String],
    repair: &command_repair::DirectCommandRepair,
    action: &str,
    original_output: &CommandOutput,
    final_output: &CommandOutput,
) {
    let command = redaction::redacted_shell_join(args);
    let identity = CommandIdentity::classify(args);
    let source = format!("passthrough:{}", identity.root);
    let record = insights::CommandRepairRecord {
        process: &identity.root,
        command_family: &identity.family,
        command: &command,
        source: &source,
        rule_id: repair.rule_id,
        action,
        original_exit_code: original_output.exit_code,
        final_exit_code: final_output.exit_code,
        original_response: &original_output.combined,
        final_response: &final_output.combined,
    };
    let _ = insights::record_command_repair(&record);
}

fn record_preflight_rewrite(
    args: &[String],
    rewrite: &command_repair::DirectCommandRewrite,
    output: &CommandOutput,
) {
    let command = redaction::redacted_shell_join(args);
    let identity = CommandIdentity::classify(args);
    let source = format!("passthrough:{}", identity.root);
    let record = insights::CommandRepairRecord {
        process: &identity.root,
        command_family: &identity.family,
        command: &command,
        source: &source,
        rule_id: rewrite.rule_id,
        action: "preflight_rewrite",
        original_exit_code: output.exit_code,
        final_exit_code: output.exit_code,
        original_response: "",
        final_response: &output.combined,
    };
    let _ = insights::record_command_repair(&record);
}

fn capture_passthrough(cmd: std::process::Command, program: &str) -> Result<CommandOutput> {
    capture_with_piped_stdin(cmd, program)
}

fn record_opportunity(args: &[String], output: &CommandOutput) {
    let projection = OPPORTUNITY_WINDOW.project(&output.combined, GENERATED_LINE_PREVIEW_CHARS);
    if projection.text == output.combined {
        return;
    }
    let projected_metrics = TextMetrics::from_text(&projection.text);
    let command = redaction::redacted_shell_join(args);
    let identity = CommandIdentity::classify(args);
    let source = format!("passthrough:{}", identity.root);
    let record = CommandOpportunityRecord {
        process: &identity.root,
        command_family: &identity.family,
        command: &command,
        source: &source,
        strategy: projection_strategy(&projection),
        confidence: projection_confidence(&projection),
        raw: output.observation(source.as_str()).metrics,
        projected: projected_metrics,
    };
    let _ = insights::record_command_opportunity(&record);
}

fn projection_confidence(projection: &ProjectedOutput) -> OpportunityConfidence {
    match (projection.line_windowed, projection.generated_lines_bounded) {
        (false, true) => OpportunityConfidence::High,
        (true, true) => OpportunityConfidence::Medium,
        _ => OpportunityConfidence::Low,
    }
}

fn projection_strategy(projection: &ProjectedOutput) -> &'static str {
    match (projection.line_windowed, projection.generated_lines_bounded) {
        (true, true) => COMBINED_STRATEGY,
        (false, true) => GENERATED_LINE_STRATEGY,
        _ => LINE_WINDOW_STRATEGY,
    }
}
