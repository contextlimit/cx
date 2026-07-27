use anyhow::Result;

use crate::support::runner::{append_failure_hint, capture, ProxyOutcome};
use crate::support::utils::resolved_command;

use super::diff::run_diff;

pub fn run_conflict_diff(args: &[String]) -> Result<ProxyOutcome> {
    let options = parse_conflict_diff_args(args)?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;
    let mut raw_output = String::new();

    for path in &options.paths {
        let mut diff_args = options.diff_args.clone();
        diff_args.push(format!(":{}:{path}", options.left_stage));
        diff_args.push(format!(":{}:{path}", options.right_stage));
        let outcome = run_diff(&diff_args)?;
        if options.paths.len() > 1 {
            stdout.push_str(&format!("--- {path} ---\n"));
        }
        stdout.push_str(outcome.stdout.trim_end());
        if !outcome.stdout.trim_end().is_empty() {
            stdout.push('\n');
        }
        if !outcome.stderr.trim_end().is_empty() {
            stderr.push_str(outcome.stderr.trim_end());
            stderr.push('\n');
        }
        if let Some(observation) = outcome.observation {
            if let Some(response) = observation.response {
                raw_output.push_str(response.trim_end());
                raw_output.push('\n');
            }
        }
        if outcome.exit_code != 0 && exit_code == 0 {
            exit_code = outcome.exit_code;
        }
    }

    Ok(ProxyOutcome {
        stdout: stdout.trim_end().to_string(),
        stderr: stderr.trim_end().to_string(),
        exit_code,
        observation: None,
    }
    .with_raw_output("git conflict-diff", raw_output.trim_end()))
}

pub fn run_evidence_diff(args: &[String]) -> Result<ProxyOutcome> {
    let options = parse_evidence_diff_args(args)?;
    let mut cmd = resolved_command("git");
    match options.revision {
        EvidenceDiffRevision::Commit(commit) => {
            if commit_has_parent(&commit)? {
                cmd.args(["diff", "--no-ext-diff", "--no-color"]);
                cmd.arg(format!("{commit}^..{commit}"));
            } else {
                cmd.args([
                    "show",
                    "--format=",
                    "--no-ext-diff",
                    "--no-color",
                    "--patch",
                ]);
                cmd.arg(commit);
            }
        }
        EvidenceDiffRevision::Range(range) => {
            cmd.args(["diff", "--no-ext-diff", "--no-color"]);
            cmd.arg(range);
        }
    }
    if !options.paths.is_empty() {
        cmd.arg("--");
        cmd.args(&options.paths);
    }

    let mut output = capture(cmd, "git evidence-diff")?;
    let exit_code = output.exit_code;
    let failure_hint = if exit_code == 0 {
        None
    } else {
        output.failure_artifact_hint("git")
    };
    let observation = output.observation("git evidence-diff");
    Ok(ProxyOutcome {
        stdout: append_failure_hint(output.stdout, failure_hint.as_deref()),
        stderr: output.stderr.trim_end().to_string(),
        exit_code,
        observation: None,
    }
    .with_observation(observation))
}

fn commit_has_parent(commit: &str) -> Result<bool> {
    let mut cmd = resolved_command("git");
    cmd.args(["rev-parse", "--verify", "--quiet"]);
    cmd.arg(format!("{commit}^"));
    let output = capture(cmd, "git evidence-diff parent probe")?;
    Ok(output.exit_code == 0)
}

#[derive(Debug)]
pub(super) struct ConflictDiffOptions {
    pub(super) left_stage: String,
    pub(super) right_stage: String,
    pub(super) diff_args: Vec<String>,
    pub(super) paths: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EvidenceDiffOptions {
    pub(super) revision: EvidenceDiffRevision,
    pub(super) paths: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum EvidenceDiffRevision {
    Commit(String),
    Range(String),
}

pub(super) fn parse_conflict_diff_args(args: &[String]) -> Result<ConflictDiffOptions> {
    let mut left_stage = "2".to_string();
    let mut right_stage = "3".to_string();
    let mut diff_args = Vec::new();
    let mut paths = Vec::new();
    let mut index = 0usize;
    let mut after_separator = false;
    while index < args.len() {
        let arg = &args[index];
        if after_separator {
            paths.push(arg.clone());
            index += 1;
            continue;
        }
        match arg.as_str() {
            "--" => {
                after_separator = true;
                index += 1;
            }
            "--stat" | "--numstat" | "--shortstat" | "--no-compact" => {
                diff_args.push(arg.clone());
                index += 1;
            }
            "--stage" => {
                let Some(value) = args.get(index + 1) else {
                    anyhow::bail!("`git conflict-diff --stage` requires a value like `2:3`");
                };
                (left_stage, right_stage) = parse_stage_pair(value)?;
                index += 2;
            }
            value if value.starts_with("--stage=") => {
                (left_stage, right_stage) = parse_stage_pair(&value["--stage=".len()..])?;
                index += 1;
            }
            value if value.starts_with('-') => {
                anyhow::bail!("unsupported cx git conflict-diff option `{value}`");
            }
            _ => {
                paths.push(arg.clone());
                index += 1;
            }
        }
    }
    if paths.is_empty() {
        anyhow::bail!("`git conflict-diff` requires at least one path");
    }
    Ok(ConflictDiffOptions {
        left_stage,
        right_stage,
        diff_args,
        paths,
    })
}

pub(super) fn parse_evidence_diff_args(args: &[String]) -> Result<EvidenceDiffOptions> {
    let mut revision = None;
    let mut paths = Vec::new();
    let mut after_separator = false;
    for arg in args {
        if after_separator {
            paths.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_separator = true;
            continue;
        }
        if arg.starts_with('-') {
            anyhow::bail!(
                "`git evidence-diff` accepts an optional commit or range followed by `-- <paths...>`"
            );
        }
        if revision.is_some() {
            anyhow::bail!(
                "`git evidence-diff` accepts only one commit/range before `-- <paths...>`"
            );
        }
        revision = Some(arg.clone());
    }

    let revision = revision.unwrap_or_else(|| "HEAD".to_string());
    let revision = if revision.contains("..") {
        EvidenceDiffRevision::Range(revision)
    } else {
        EvidenceDiffRevision::Commit(revision)
    };
    Ok(EvidenceDiffOptions { revision, paths })
}

fn parse_stage_pair(value: &str) -> Result<(String, String)> {
    let Some((left, right)) = value.split_once(':') else {
        anyhow::bail!("stage pair `{value}` must look like `2:3`");
    };
    if left.is_empty()
        || right.is_empty()
        || !left.chars().all(|ch| ch.is_ascii_digit())
        || !right.chars().all(|ch| ch.is_ascii_digit())
    {
        anyhow::bail!("stage pair `{value}` must use numeric stages");
    }
    Ok((left.to_string(), right.to_string()))
}
