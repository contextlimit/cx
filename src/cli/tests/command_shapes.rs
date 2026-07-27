use super::*;
use clap::error::ErrorKind;

#[test]
fn version_reports_package_and_build_revision() {
    let error = try_parse_from_cx_args(["cx", "--version"]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    let rendered = error.to_string();
    assert!(rendered.contains(env!("CARGO_PKG_VERSION")));
    assert!(rendered.contains(env!("CX_BUILD_REVISION")));
}

#[test]
fn cli_accepts_file_and_search_proxy_families() {
    assert!(matches!(
        parse_cli(["cx", "git", "status", "--short"]).command,
        Command::Git {
            command: GitProxyCommand::Status { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "diff", "--stat"]).command,
        Command::Diff { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "read", "--smart", "README.md"]).command,
        Command::Read { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "grep", "needle", "src"]).command,
        Command::Grep { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "rg", "needle", "src"]).command,
        Command::Grep { .. }
    ));
    let Command::Grep {
        no_compact, terms, ..
    } = parse_cli(["cx", "rg", "--no-compact", "-n", "needle", "src"]).command
    else {
        panic!("expected rg wrapper");
    };
    assert!(no_compact);
    assert_eq!(terms, ["needle", "src"]);
    assert!(matches!(
        parse_cli(["cx", "ls"]).command,
        Command::Ls { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "find", "src", "-type", "f"]).command,
        Command::Find { .. }
    ));
    let Command::Ps { args } = parse_cli(["cx", "ps", "-axo", "pid,ppid,etime,command"]).command
    else {
        panic!("expected ps wrapper");
    };
    assert_eq!(args, ["-axo", "pid,ppid,etime,command"]);
    let Command::Sh { no_compact, args } =
        parse_cli(["cx", "sh", "--no-compact", "-lc", "printf ok"]).command
    else {
        panic!("expected shell wrapper");
    };
    assert!(no_compact);
    assert_eq!(args, ["-lc", "printf ok"]);
    assert!(matches!(
        parse_cli(["cx", "git", "conflict-diff", "--stat", "src/lib.rs"]).command,
        Command::Git {
            command: GitProxyCommand::ConflictDiff { .. }
        }
    ));
    assert!(matches!(
        parse_cli([
            "cx",
            "git",
            "evidence-diff",
            "HEAD~1..HEAD",
            "--",
            "src/lib.rs"
        ])
        .command,
        Command::Git {
            command: GitProxyCommand::EvidenceDiff { .. }
        }
    ));
}

#[test]
fn git_pathspec_separators_survive_clap_parsing() {
    let cases = [
        (
            vec!["cx", "diff", "--", "src/lib.rs"],
            vec!["--", "src/lib.rs"],
        ),
        (
            vec!["cx", "git", "status", "--short", "--", "src/lib.rs"],
            vec!["--short", "--", "src/lib.rs"],
        ),
        (
            vec!["cx", "git", "diff", "--", "src/lib.rs"],
            vec!["--", "src/lib.rs"],
        ),
        (
            vec!["cx", "git", "log", "-1", "--", "src/lib.rs"],
            vec!["-1", "--", "src/lib.rs"],
        ),
        (
            vec!["cx", "git", "show", "HEAD", "--", "src/lib.rs"],
            vec!["HEAD", "--", "src/lib.rs"],
        ),
        (
            vec![
                "cx",
                "git",
                "evidence-diff",
                "HEAD~1..HEAD",
                "--",
                "src/lib.rs",
            ],
            vec!["HEAD~1..HEAD", "--", "src/lib.rs"],
        ),
        (
            vec!["cx", "--", "git", "diff", "--", "src/lib.rs"],
            vec!["--", "src/lib.rs"],
        ),
    ];

    for (raw, expected) in cases {
        let cli = parse_from_cx_args(raw);
        let args = match cli.command {
            Command::Diff { args } => args,
            Command::Git {
                command:
                    GitProxyCommand::Status { args }
                    | GitProxyCommand::Diff { args }
                    | GitProxyCommand::Log { args }
                    | GitProxyCommand::Show { args }
                    | GitProxyCommand::EvidenceDiff { args }
                    | GitProxyCommand::ConflictDiff { args },
            } => args,
            other => panic!("unexpected command: {other:?}"),
        };
        assert_eq!(args, expected);
    }
}

#[test]
fn cli_accepts_read_like_proxy_families() {
    assert!(matches!(
        parse_cli(["cx", "cat", "README.md"]).command,
        Command::Cat { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "head", "-n", "5", "README.md"]).command,
        Command::Head { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "tail", "-n", "+12", "README.md"]).command,
        Command::Tail { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "sed", "-n", "1,8p", "README.md"]).command,
        Command::Sed { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "nl", "-ba", "README.md"]).command,
        Command::Nl { .. }
    ));
}

#[test]
fn cli_accepts_insights_proxy_family() {
    assert!(matches!(
        parse_cli(["cx", "insights", "summary"]).command,
        Command::Insights {
            command: InsightsCommand::Summary { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "top", "--sort", "lines", "--limit", "5"]).command,
        Command::Insights {
            command: InsightsCommand::Top { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "largest", "--sort", "chars", "--limit", "7"]).command,
        Command::Insights {
            command: InsightsCommand::Largest { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "daily", "--limit", "30"]).command,
        Command::Insights {
            command: InsightsCommand::Daily { .. }
        }
    ));
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "expansions",
            "--root",
            "git",
            "--limit",
            "8"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::Expansions { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "presentation"]).command,
        Command::Insights {
            command: InsightsCommand::Presentation { .. }
        }
    ));
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "impact",
            "--limit",
            "4",
            "--context-window-tokens",
            "128000"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::Impact { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "recommend", "--limit", "4"]).command,
        Command::Insights {
            command: InsightsCommand::Recommend { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "opportunities", "--limit", "4"]).command,
        Command::Insights {
            command: InsightsCommand::Opportunities { .. }
        }
    ));
}

#[test]
fn cli_accepts_insights_archive_and_failure_sorting() {
    let Command::Insights {
        command: InsightsCommand::ArchiveSummary { archives, limit },
    } = parse_cli([
        "cx",
        "insights",
        "archive-summary",
        "--archive",
        "project-a.sqlite",
        "--archive",
        "project-b.sqlite",
        "--limit",
        "7",
    ])
    .command
    else {
        panic!("expected insights archive-summary");
    };
    assert_eq!(
        archives,
        [
            PathBuf::from("project-a.sqlite"),
            PathBuf::from("project-b.sqlite")
        ]
    );
    assert_eq!(limit, 7);
    assert!(matches!(
        parse_cli(["cx", "insights", "top", "--sort", "failures"]).command,
        Command::Insights {
            command: InsightsCommand::Top { .. }
        }
    ));
}

#[test]
fn cli_accepts_insights_routing_filters() {
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "routing",
            "--root",
            "git",
            "--command",
            "git branch",
            "--limit",
            "4"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::Routing { .. }
        }
    ));
}

#[test]
fn cli_accepts_insights_reporting_family() {
    assert!(matches!(
        parse_cli(["cx", "insights", "failures", "--limit", "3"]).command,
        Command::Insights {
            command: InsightsCommand::Failures { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "reports", "--limit", "3"]).command,
        Command::Insights {
            command: InsightsCommand::Reports {
                status: InsightsReportStatusFilter::All,
                ..
            }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "reports", "--status", "native-parity"]).command,
        Command::Insights {
            command: InsightsCommand::Reports {
                status: InsightsReportStatusFilter::NativeParity,
                ..
            }
        }
    ));
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "report-update",
            "30",
            "--status",
            "resolved",
            "--note",
            "fixed in the shell wrapper",
            "--revision",
            "r110"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::ReportUpdate {
                report_id: 30,
                status: InsightsReportStatus::Resolved,
                ..
            }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "dashboard", "--limit", "3"]).command,
        Command::Insights {
            command: InsightsCommand::Dashboard { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "audit", "--limit", "3"]).command,
        Command::Insights {
            command: InsightsCommand::Audit { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "audit", "--format", "json", "--limit", "3"]).command,
        Command::Insights {
            command: InsightsCommand::Audit { .. }
        }
    ));
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "settings",
            "--set",
            "record_invocations=true"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::Settings { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "export", "--format", "json", "--limit", "3"]).command,
        Command::Insights {
            command: InsightsCommand::Export { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "export", "--format", "csv"]).command,
        Command::Insights {
            command: InsightsCommand::Export { .. }
        }
    ));
}

#[test]
fn cli_accepts_insights_report_denial_and_triage_family() {
    assert!(matches!(
        parse_cli(["cx", "insights", "reports", "--status", "denied"]).command,
        Command::Insights {
            command: InsightsCommand::Reports {
                status: InsightsReportStatusFilter::Denied,
                ..
            }
        }
    ));
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "report-update",
            "31",
            "--status",
            "denied",
            "--reason",
            "duplicate",
            "--related-report-id",
            "30",
            "--note",
            "exact duplicate"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::ReportUpdate {
                report_id: 31,
                status: InsightsReportStatus::Denied,
                reason: Some(InsightsReportDenialReason::Duplicate),
                related_report_id: Some(30),
                ..
            }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "report-triage"]).command,
        Command::Insights {
            command: InsightsCommand::ReportTriage {
                apply: false,
                format: InsightsReportTriageFormat::Text,
                limit: 25,
            }
        }
    ));
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "report-triage",
            "--apply",
            "--format",
            "json",
            "--limit",
            "5"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::ReportTriage {
                apply: true,
                format: InsightsReportTriageFormat::Json,
                limit: 5,
            }
        }
    ));
}

#[test]
fn cli_accepts_insights_filter_flags() {
    assert!(matches!(
        parse_cli([
            "cx",
            "insights",
            "largest",
            "--root",
            "git",
            "--command",
            "git diff"
        ])
        .command,
        Command::Insights {
            command: InsightsCommand::Largest { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "failures", "--level", "root", "--root", "git"]).command,
        Command::Insights {
            command: InsightsCommand::Failures { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "reports", "--level", "root", "--root", "git"]).command,
        Command::Insights {
            command: InsightsCommand::Reports { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "dashboard", "--root", "git"]).command,
        Command::Insights {
            command: InsightsCommand::Dashboard { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "audit", "--root", "git"]).command,
        Command::Insights {
            command: InsightsCommand::Audit { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "insights", "export", "--format", "json", "--root", "grep"]).command,
        Command::Insights {
            command: InsightsCommand::Export { .. }
        }
    ));
}

#[test]
fn cli_accepts_report_family_with_hyphenated_reported_command() {
    match parse_cli([
        "cx",
        "report",
        "--",
        "cx",
        "grep",
        "-n",
        "route|path",
        "src/app.mjs",
    ])
    .command
    {
        Command::Report { args } => {
            assert_eq!(
                args,
                vec![
                    "cx".to_string(),
                    "grep".to_string(),
                    "-n".to_string(),
                    "route|path".to_string(),
                    "src/app.mjs".to_string(),
                ]
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn cli_accepts_test_and_language_proxy_families() {
    assert!(matches!(
        parse_cli(["cx", "pytest", "-q"]).command,
        Command::Pytest { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "cargo", "test", "crate_name"]).command,
        Command::Cargo {
            command: CargoProxyCommand::Test { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "go", "test", "./..."]).command,
        Command::Go {
            command: GoProxyCommand::Test { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "tsc", "--noEmit"]).command,
        Command::Tsc { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "node", "--check", "widget.jsx"]).command,
        Command::Node { .. }
    ));
    assert!(matches!(
        parse_cli(["cx", "cmake", "build", "build-dir"]).command,
        Command::Cmake {
            command: CmakeProxyCommand::Build { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "cmake", "--build", "build-dir"]).command,
        Command::Cmake {
            command: CmakeProxyCommand::Build { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "ctest", "--output-on-failure"]).command,
        Command::Ctest { .. }
    ));
}

#[test]
fn cli_accepts_container_proxy_families() {
    assert!(matches!(
        parse_cli(["cx", "docker", "ps"]).command,
        Command::Docker {
            command: DockerProxyCommand::Ps { .. }
        }
    ));
    assert!(matches!(
        parse_cli(["cx", "kubectl", "logs", "pod-1"]).command,
        Command::Kubectl {
            command: KubectlProxyCommand::Logs { .. }
        }
    ));
}

#[test]
fn node_command_accepts_multiple_files() {
    match parse_cli([
        "cx",
        "node",
        "--check",
        "ChatBrowser.jsx",
        "ChatCollaboration.jsx",
        "Chat.jsx",
    ])
    .command
    {
        Command::Node { args } => {
            assert_eq!(
                args,
                vec![
                    "--check".to_string(),
                    "ChatBrowser.jsx".to_string(),
                    "ChatCollaboration.jsx".to_string(),
                    "Chat.jsx".to_string(),
                ]
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn diff_pathspec_separator_is_retained() {
    match parse_cli(["cx", "diff", "--", "a.cpp", "b.cpp"]).command {
        Command::Diff { args } => {
            assert_eq!(
                args,
                vec!["--".to_string(), "a.cpp".to_string(), "b.cpp".to_string()]
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn grep_accepts_pattern_starting_with_double_dash() {
    match parse_cli(["cx", "grep", "--generate", "service_main.cpp"]).command {
        Command::Grep {
            patterns, terms, ..
        } => {
            assert_eq!(patterns, vec!["--generate".to_string()]);
            assert_eq!(terms, vec!["service_main.cpp".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn rg_alias_accepts_pattern_starting_with_double_dash() {
    match parse_cli(["cx", "rg", "--generate", "service_main.cpp"]).command {
        Command::Grep {
            patterns,
            extended_regexp,
            terms,
            ..
        } => {
            assert!(extended_regexp);
            assert_eq!(patterns, vec!["--generate".to_string()]);
            assert_eq!(terms, vec!["service_main.cpp".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn rg_alias_enables_extended_regexp_automatically() {
    match parse_cli(["cx", "rg", "foo|bar", "src"]).command {
        Command::Grep {
            extended_regexp,
            terms,
            ..
        } => {
            assert!(extended_regexp);
            assert_eq!(terms, vec!["foo|bar".to_string(), "src".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn rg_accepts_no_ignore_before_fixed_pattern_and_trailing_glob() {
    match parse_cli([
        "cx",
        "rg",
        "--no-ignore",
        "-n",
        "-i",
        "-F",
        "evolution",
        "app/sample-ui/build-web",
        "-g",
        "*.js",
    ])
    .command
    {
        Command::Grep {
            no_ignore,
            ignore_case,
            fixed_strings,
            globs,
            terms,
            ..
        } => {
            assert!(no_ignore);
            assert!(ignore_case);
            assert!(fixed_strings);
            assert_eq!(globs, vec!["*.js".to_string()]);
            assert_eq!(
                terms,
                vec![
                    "evolution".to_string(),
                    "app/sample-ui/build-web".to_string(),
                ]
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_from_cx_args_preserves_normalized_raw_args() {
    let cli = parse_from_cx_args([
        "/Users/example/.local/bin/cx",
        "rg",
        "foo|bar",
        "src/app file.rs",
    ]);
    assert_eq!(
        cli.raw_args,
        vec![
            "cx".to_string(),
            "rg".to_string(),
            "foo|bar".to_string(),
            "src/app file.rs".to_string(),
        ]
    );
}

#[test]
fn grep_preserves_known_flags_before_double_dash_pattern() {
    match parse_cli([
        "cx",
        "grep",
        "--glob",
        "*.cpp",
        "--max-results",
        "5",
        "--generate",
        "service_main.cpp",
    ])
    .command
    {
        Command::Grep {
            patterns,
            globs,
            max_results,
            terms,
            ..
        } => {
            assert_eq!(patterns, vec!["--generate".to_string()]);
            assert_eq!(globs, vec!["*.cpp".to_string()]);
            assert_eq!(max_results, Some(5));
            assert_eq!(terms, vec!["service_main.cpp".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn grep_accepts_explicit_regexp_for_option_named_patterns() {
    match parse_cli(["cx", "grep", "-e", "--max-results", "service_main.cpp"]).command {
        Command::Grep {
            patterns, terms, ..
        } => {
            assert_eq!(patterns, vec!["--max-results".to_string()]);
            assert_eq!(terms, vec!["service_main.cpp".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn grep_accepts_multiple_explicit_regexps() {
    match parse_cli(["cx", "rg", "-e", "`", "-e", "${", "dockview_styles.js"]).command {
        Command::Grep {
            patterns, terms, ..
        } => {
            assert_eq!(patterns, vec!["`".to_string(), "${".to_string()]);
            assert_eq!(terms, vec!["dockview_styles.js".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn rg_accepts_repeated_idempotent_line_number_flags() {
    match parse_cli([
        "cx",
        "rg",
        "-n",
        "DispatchVoiceContractCommandJson",
        "-n",
        "packages/voice_dev/src/contract.cpp",
    ])
    .command
    {
        Command::Grep {
            line_numbers,
            extended_regexp,
            terms,
            ..
        } => {
            assert!(line_numbers);
            assert!(extended_regexp);
            assert_eq!(
                terms,
                vec![
                    "DispatchVoiceContractCommandJson".to_string(),
                    "packages/voice_dev/src/contract.cpp".to_string(),
                ]
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn grep_does_not_dedupe_a_flag_shaped_explicit_pattern() {
    match parse_cli(["cx", "grep", "-e", "-n", "-n", "src"]).command {
        Command::Grep {
            patterns,
            line_numbers,
            terms,
            ..
        } => {
            assert_eq!(patterns, vec!["-n".to_string()]);
            assert!(line_numbers);
            assert_eq!(terms, vec!["src".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn help_mentions_proxy_commands_only() {
    let mut command = Cli::command();
    let mut help = Vec::new();
    command.write_long_help(&mut help).unwrap();
    let help = String::from_utf8(help).unwrap();
    for expected in [
        "git", "diff", "read", "grep", "rg", "ls", "pytest", "cargo", "go", "tsc", "node", "cmake",
        "ctest", "find", "docker", "kubectl", "report",
    ] {
        assert!(help.contains(expected), "missing help entry for {expected}");
    }
    for unexpected in [
        "mcp",
        "daemon",
        "consult",
        "research",
        "memory",
        "studio",
        "understand",
    ] {
        assert!(
            !help.contains(unexpected),
            "unexpected legacy command leaked into help: {unexpected}"
        );
    }
}
