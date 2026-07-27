use anyhow::{bail, Result};

use crate::commands::command_identity::CommandIdentity;
use crate::support::insights::{self, CommandReportRecord, GENERIC_COMMAND_REPORT_NOTE};
use crate::support::redaction;
use crate::support::runner::ProxyOutcome;

const ISSUE_KIND: &str = "suspicious_output";

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    if args.is_empty() {
        bail!("report requires a cx command to report");
    }

    let identity = CommandIdentity::classify(args);
    let command = redaction::redacted_shell_join(&identity.argv);
    let command_shape = redaction::argument_shape_join(&identity.argv);
    let command_shape_hash = redaction::stable_shape_hash(&command_shape);
    let diagnostic = command_diagnostic(&identity);
    let command_family = identity.family;
    let note = report_note(diagnostic);
    let receipt = insights::record_command_report(&CommandReportRecord {
        command: &command,
        command_family: &command_family,
        command_shape: &command_shape,
        command_shape_hash: &command_shape_hash,
        issue_kind: ISSUE_KIND,
        note: &note,
    })?;

    let mut output = String::from("cx report: command-quality issue\n");
    output.push_str(&format!(
        "Database: {}\n",
        insights::insights_database_path()?.display()
    ));
    if receipt.recorded {
        output.push_str(&format!(
            "Recorded: yes\nReport id: {}\nStatus: open\nFamily: {} ({} family reports, {} total reports)\n",
            receipt.id, receipt.command_family, receipt.family_reports, receipt.total_reports,
        ));
        if let Some(evidence) = insights::recent_command_reports(1)?
            .into_iter()
            .find(|report| report.id == receipt.id)
        {
            let invocation = evidence
                .invocation_id
                .map_or_else(|| "none".to_string(), |id| id.to_string());
            output.push_str(&format!(
                "Invocation: {invocation}\nEvidence: {}\nCX response: {}\nNative response: {}\nBinary: {} {}\n",
                evidence.evidence_kind,
                evidence_status(&evidence.cx_response),
                evidence_status(&evidence.native_response),
                evidence.binary_version,
                evidence.binary_revision,
            ));
            if !evidence.artifact_ref.is_empty() {
                output.push_str(&format!("Artifact: {}\n", evidence.artifact_ref));
            }
        }
    } else {
        output.push_str("Recorded: no (CX_DISABLE_INSIGHTS is set)\n");
        output.push_str(&format!("Family: {command_family}\n"));
    }
    output.push_str(&format!(
        "Issue kind: {ISSUE_KIND}\nCommand: {command}\nNote: {note}"
    ));
    Ok(ProxyOutcome::success(output))
}

fn evidence_status(value: &str) -> &'static str {
    if value.is_empty() {
        "not recorded"
    } else {
        "recorded"
    }
}

fn report_note(diagnostic: Option<&'static str>) -> String {
    if let Some(diagnostic) = diagnostic {
        format!("{GENERIC_COMMAND_REPORT_NOTE} Diagnostic: {diagnostic}")
    } else {
        GENERIC_COMMAND_REPORT_NOTE.to_string()
    }
}

fn command_diagnostic(identity: &CommandIdentity) -> Option<&'static str> {
    if identity.family == "grep basic" && has_unescaped_pipe(&identity.argv) {
        Some(
            "default `cx grep` uses basic grep-style patterns, so bare `|` is literal; use `cx grep -E` or `cx rg` for alternation.",
        )
    } else if is_nonportable_errexit_conditional(&identity.argv) {
        Some(
            "standalone `[[ ... ]]` is not a portable `set -e` assertion across Bash runtimes; CX preserves the resolved Bash behavior, so use `[[ ... ]] || exit 1` when a false condition must stop the script.",
        )
    } else {
        None
    }
}

fn is_nonportable_errexit_conditional(args: &[String]) -> bool {
    let Some(script) = shell_command_script(args) else {
        return false;
    };
    (script.contains("set -e") || script.contains("set -o errexit"))
        && script.contains("[[")
        && script.contains("]]")
        && !script.contains("if [[")
        && !script.contains("]] ||")
        && !script.contains("]]||")
        && !script.contains("]] &&")
        && !script.contains("]]&&")
}

fn shell_command_script(args: &[String]) -> Option<&str> {
    if !matches!(args.first().map(String::as_str), Some("bash" | "sh")) {
        return None;
    }
    args.iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, arg)| is_shell_command_option(arg).then(|| args.get(index + 1))?)
        .map(String::as_str)
}

fn is_shell_command_option(arg: &str) -> bool {
    arg == "-c"
        || (arg.starts_with('-')
            && !arg.starts_with("--")
            && arg
                .strip_prefix('-')
                .is_some_and(|flags| flags.contains('c')))
}

fn has_unescaped_pipe(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| contains_unescaped_pipe(arg))
}

fn contains_unescaped_pipe(value: &str) -> bool {
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '|' {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    use rusqlite::Connection;

    fn command_family(args: &[String]) -> String {
        CommandIdentity::classify(args).family
    }

    fn diagnostic(args: &[String]) -> Option<&'static str> {
        command_diagnostic(&CommandIdentity::classify(args))
    }

    #[test]
    fn report_groups_cx_grep_under_basic_grep_family() {
        let args = vec![
            "cx".to_string(),
            "grep".to_string(),
            "-n".to_string(),
            "route|path".to_string(),
            "src".to_string(),
        ];
        assert_eq!(command_family(&args), "grep basic");
        assert!(diagnostic(&args).unwrap().contains("bare `|`"));
        assert_eq!(
            redaction::redacted_shell_join(&args),
            "cx grep -n 'route|path' src".to_string()
        );
    }

    #[test]
    fn report_does_not_warn_for_extended_or_rg_patterns() {
        let grep_args = vec![
            "cx".to_string(),
            "grep".to_string(),
            "-E".to_string(),
            "route|path".to_string(),
        ];
        let rg_args = vec!["cx".to_string(), "rg".to_string(), "route|path".to_string()];
        assert_eq!(diagnostic(&grep_args), None);
        assert_eq!(diagnostic(&rg_args), None);
        assert_eq!(command_family(&grep_args), "grep extended");
        assert_eq!(command_family(&rg_args), "rg extended");
    }

    #[test]
    fn report_explains_nonportable_errexit_conditional() {
        let args = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "set -e; x=7204; [[ \"$x\" == \"7211\" ]]; printf should-not-print".to_string(),
        ];
        let diagnostic = diagnostic(&args).unwrap();

        assert!(diagnostic.contains("not a portable `set -e` assertion"));
        assert!(diagnostic.contains("[[ ... ]] || exit 1"));
        assert_eq!(command_family(&args), "sh");
    }

    #[test]
    fn report_does_not_warn_for_explicitly_guarded_shell_conditional() {
        let args = vec![
            "cx".to_string(),
            "--".to_string(),
            "bash".to_string(),
            "-lc".to_string(),
            "set -e; [[ -d src ]] || exit 1; printf ok".to_string(),
        ];

        assert_eq!(diagnostic(&args), None);
    }

    #[test]
    fn report_classifies_fixed_and_file_searches_with_explicit_separator() {
        assert_eq!(
            command_family(&[
                "cx".to_string(),
                "--".to_string(),
                "rg".to_string(),
                "-F".to_string(),
                "needle".to_string(),
                "src".to_string(),
            ]),
            "rg fixed"
        );
        assert_eq!(
            command_family(&[
                "cx".to_string(),
                "grep".to_string(),
                "--files".to_string(),
                "src".to_string(),
            ]),
            "grep files"
        );
    }

    #[test]
    fn report_extracts_two_token_families() {
        assert_eq!(
            command_family(&["cx".to_string(), "git".to_string(), "diff".to_string()]),
            "git diff"
        );
        assert_eq!(
            command_family(&[
                "cx".to_string(),
                "node".to_string(),
                "--check".to_string(),
                "app.js".to_string()
            ]),
            "node check"
        );
    }

    #[test]
    fn report_classification_never_uses_cx_or_separator_wrapper_roots() {
        for (args, expected) in [
            (vec!["cx", "--", "git", "diff", "--stat"], "git diff"),
            (
                vec!["--", "cx", "--", "node", "--test", "test.mjs"],
                "node test",
            ),
            (vec!["cx -- npm run build"], "npm build"),
        ] {
            let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
            let family = command_family(&args);
            assert_eq!(family, expected);
            assert_ne!(family, "cx");
            assert_ne!(family, "--");
        }
    }

    #[test]
    fn run_records_report_in_insights_database() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        crate::support::test_support::with_env_vars(
            &[
                (
                    "CX_INSIGHTS_DB_PATH",
                    Some(db_path.to_string_lossy().as_ref()),
                ),
                ("CX_DISABLE_INSIGHTS", None),
            ],
            || {
                let outcome = run(&[
                    "cx".to_string(),
                    "grep".to_string(),
                    "route|path".to_string(),
                    "src".to_string(),
                ])
                .unwrap();
                assert_eq!(outcome.exit_code, 0);
                assert!(outcome.stdout.contains("Recorded: yes"));
                assert!(outcome.stdout.contains("Family: grep basic"));
                assert!(outcome.stdout.contains("bare `|` is literal"));

                let reports = insights::recent_command_reports(5).unwrap();
                assert_eq!(reports.len(), 1);
                assert_eq!(reports[0].command_family, "grep basic");
                assert!(reports[0].note.contains("Diagnostic:"));
            },
        );
    }

    #[test]
    fn run_records_canonical_family_instead_of_wrapper_roots() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        crate::support::test_support::with_env_vars(
            &[
                (
                    "CX_INSIGHTS_DB_PATH",
                    Some(db_path.to_string_lossy().as_ref()),
                ),
                ("CX_DISABLE_INSIGHTS", None),
            ],
            || {
                let outcome = run(&["cx -- git diff --stat".to_string()]).unwrap();
                assert!(outcome.stdout.contains("Family: git diff"));

                let outcome = run(&[
                    "cx".to_string(),
                    "--".to_string(),
                    "jq".to_string(),
                    ".items".to_string(),
                ])
                .unwrap();
                assert!(outcome.stdout.contains("Family: passthrough jq"));

                let reports = insights::recent_command_reports(5).unwrap();
                assert_eq!(reports.len(), 2);
                assert_eq!(reports[0].command_family, "passthrough jq");
                assert_eq!(reports[1].command_family, "git diff");
                assert!(reports
                    .iter()
                    .all(|report| !matches!(report.command_family.as_str(), "cx" | "--")));
            },
        );
    }

    #[test]
    fn run_waits_for_short_concurrent_database_writer() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        crate::support::test_support::with_env_vars(
            &[
                (
                    "CX_INSIGHTS_DB_PATH",
                    Some(db_path.to_string_lossy().as_ref()),
                ),
                ("CX_DISABLE_INSIGHTS", None),
            ],
            || {
                run(&["initialize report database".to_string()]).unwrap();
                let locker = Connection::open(&db_path).unwrap();
                locker
                    .execute_batch(
                        "BEGIN EXCLUSIVE;
                         UPDATE settings
                         SET updated_at_ms = updated_at_ms
                         WHERE key = 'record_invocations';",
                    )
                    .unwrap();
                let release = thread::spawn(move || {
                    thread::sleep(Duration::from_millis(250));
                    locker.execute_batch("COMMIT;").unwrap();
                });

                let started = Instant::now();
                let outcome = run(&["concurrent report".to_string()]).unwrap();
                let elapsed = started.elapsed();
                release.join().unwrap();

                assert!(outcome.stdout.contains("Recorded: yes"));
                assert!(outcome.stdout.contains("Report id: 2"));
                assert!(elapsed >= Duration::from_millis(150));
            },
        );
    }

    #[test]
    fn run_redacts_reported_command_before_display_and_storage() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("db.sqlite");
        crate::support::test_support::with_env_vars(
            &[
                (
                    "CX_INSIGHTS_DB_PATH",
                    Some(db_path.to_string_lossy().as_ref()),
                ),
                ("CX_DISABLE_INSIGHTS", None),
            ],
            || {
                let outcome = run(&[
                    "cx".to_string(),
                    "grep".to_string(),
                    "--token".to_string(),
                    "sk-proj-abcdefghijklmnopqrstuvwxyz".to_string(),
                    "src".to_string(),
                ])
                .unwrap();
                assert!(outcome
                    .stdout
                    .contains("Command: grep --token '[REDACTED]' src"));
                assert!(!outcome
                    .stdout
                    .contains("sk-proj-abcdefghijklmnopqrstuvwxyz"));

                let reports = insights::recent_command_reports(5).unwrap();
                assert_eq!(reports[0].command, "grep --token '[REDACTED]' src");
                assert!(!reports[0].command_shape.contains("sk-proj-"));
                assert_eq!(reports[0].evidence_kind, "no-match");
            },
        );
    }
}
