use super::*;
use clap::error::ErrorKind;

#[test]
fn unsupported_passthrough_is_enabled_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args(["cx", "uname", "-a"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(args, ["uname", "-a"]);
            assert!(!db_path.exists());
        },
    );
}

#[test]
fn explicit_unknown_command_explains_how_to_reenable_passthrough() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            crate::support::insights::set_insight_setting(
                "passthrough_unsupported_commands",
                "false",
            )
            .unwrap();
            let error = try_parse_from_cx_args(["cx", "--", "uptime"]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
            let rendered = error.to_string();
            assert!(rendered.contains("unsupported command passthrough is disabled"));
            assert!(rendered
                .contains("cx insights settings --set passthrough_unsupported_commands=true"));
        },
    );
}

#[test]
fn unsupported_passthrough_accepts_unknown_command_when_enabled() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli = try_parse_from_cx_args(["cx", "uname", "-a"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(args, ["uname", "-a"]);
            assert_eq!(cli.raw_args, ["cx", "uname", "-a"]);
        },
    );
}

#[test]
fn unsupported_passthrough_accepts_explicit_separator_when_enabled() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli = try_parse_from_cx_args(["cx", "--", "sqlite3", "--version"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(args, ["sqlite3", "--version"]);
            assert_eq!(cli.raw_args, ["cx", "--", "sqlite3", "--version"]);
        },
    );
}

#[test]
fn explicit_separator_dispatches_supported_command_without_passthrough() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args(["cx", "--", "git", "diff", "--stat"]).unwrap();
            let Command::Git {
                command: GitProxyCommand::Diff { args },
            } = cli.command
            else {
                panic!("expected git diff wrapper");
            };
            assert_eq!(args, ["--stat"]);
            assert_eq!(cli.raw_args, ["cx", "git", "diff", "--stat"]);

            let cli = try_parse_from_cx_args(["cx", "--", "git", "evidence-diff"]).unwrap();
            assert!(matches!(
                cli.command,
                Command::Git {
                    command: GitProxyCommand::EvidenceDiff { .. }
                }
            ));
            assert_eq!(cli.raw_args, ["cx", "git", "evidence-diff"]);

            let cli = try_parse_from_cx_args(["cx", "--", "git", "evidence-diff", "HEAD^..HEAD"])
                .unwrap();
            let Command::Git {
                command: GitProxyCommand::EvidenceDiff { args },
            } = cli.command
            else {
                panic!("expected git evidence-diff wrapper");
            };
            assert_eq!(args, ["HEAD^..HEAD"]);
            assert_eq!(cli.raw_args, ["cx", "git", "evidence-diff", "HEAD^..HEAD"]);
        },
    );
}

#[test]
fn explicit_separator_uses_passthrough_for_native_diff() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            for case in [
                &["cx", "--", "diff", "-qr", "left", "right"][..],
                &["cx", "--", "diff", "-u", "left", "right"][..],
            ] {
                let cli = try_parse_from_cx_args(case.iter().copied()).unwrap();
                let Command::Passthrough { args } = cli.command else {
                    panic!("expected native diff passthrough for {case:?}");
                };
                assert_eq!(args, case[2..]);
                assert_eq!(cli.raw_args, case);
            }

            let cli = try_parse_from_cx_args(["cx", "diff", "--stat"]).unwrap();
            assert!(matches!(cli.command, Command::Diff { .. }));
        },
    );
}

#[test]
fn explicit_separator_bash_preserves_cx_no_compact_option() {
    let cli =
        try_parse_from_cx_args(["cx", "--", "bash", "--no-compact", "-lc", "printf ok"]).unwrap();
    let Command::Sh { no_compact, args } = cli.command else {
        panic!("expected shell wrapper");
    };
    assert!(no_compact);
    assert_eq!(args, ["-lc", "printf ok"]);
    assert_eq!(
        cli.raw_args,
        ["cx", "--", "bash", "--no-compact", "-lc", "printf ok"]
    );

    let cli = try_parse_from_cx_args(["cx", "--", "bash", "--no-compact"]).unwrap();
    let Command::Sh { no_compact, args } = cli.command else {
        panic!("expected stdin shell wrapper");
    };
    assert!(no_compact);
    assert!(args.is_empty());
}

#[test]
fn explicit_separator_dispatches_native_cmake_build_without_passthrough() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args([
                "cx",
                "--",
                "cmake",
                "--build",
                "build-web",
                "--target",
                "sample-ui",
                "-j8",
            ])
            .unwrap();
            let Command::Cmake {
                command: CmakeProxyCommand::Build { args },
            } = cli.command
            else {
                panic!("expected cmake build wrapper");
            };
            assert_eq!(args, ["build-web", "--target", "sample-ui", "-j8"]);
            assert_eq!(
                cli.raw_args,
                [
                    "cx",
                    "cmake",
                    "--build",
                    "build-web",
                    "--target",
                    "sample-ui",
                    "-j8"
                ]
            );
        },
    );
}

#[test]
fn explicit_separator_dispatches_ps_without_passthrough() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args(["cx", "--", "ps", "-axo", "pid,ppid,etime,command"])
                .unwrap();
            let Command::Ps { args } = cli.command else {
                panic!("expected ps wrapper");
            };
            assert_eq!(args, ["-axo", "pid,ppid,etime,command"]);
            assert_eq!(cli.raw_args, ["cx", "ps", "-axo", "pid,ppid,etime,command"]);
        },
    );
}

#[test]
fn explicit_separator_dispatches_supported_grep_when_parse_is_clear() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli = try_parse_from_cx_args(["cx", "--", "grep", "needle", "src"]).unwrap();
            assert!(matches!(cli.command, Command::Grep { .. }));
            assert_eq!(cli.raw_args, ["cx", "grep", "needle", "src"]);
        },
    );
}

#[test]
fn explicit_separator_dispatches_supported_rg_pattern_starting_with_double_dash() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args([
                "cx",
                "--",
                "rg",
                "-n",
                "--desk-(light-overlay|overlay|shadow)",
                "packages/desk_ui_v1/src",
                "-g",
                "*.js",
            ])
            .unwrap();
            let Command::Grep {
                extended_regexp,
                line_numbers,
                patterns,
                terms,
                globs,
                ..
            } = cli.command
            else {
                panic!("expected rg wrapper");
            };
            assert!(extended_regexp);
            assert!(line_numbers);
            assert_eq!(patterns, ["--desk-(light-overlay|overlay|shadow)"]);
            assert_eq!(terms, ["packages/desk_ui_v1/src"]);
            assert_eq!(globs, ["*.js"]);
            assert_eq!(
                cli.raw_args,
                [
                    "cx",
                    "rg",
                    "-n",
                    "--desk-(light-overlay|overlay|shadow)",
                    "packages/desk_ui_v1/src",
                    "-g",
                    "*.js"
                ]
            );
        },
    );
}

#[test]
fn explicit_separator_dispatches_supported_rg_no_compact() {
    let cli = try_parse_from_cx_args([
        "cx",
        "--",
        "rg",
        "--no-compact",
        "-n",
        "^test\\(",
        "tests/example.mjs",
    ])
    .unwrap();
    let Command::Grep {
        extended_regexp,
        no_compact,
        terms,
        ..
    } = cli.command
    else {
        panic!("expected rg wrapper");
    };
    assert!(extended_regexp);
    assert!(no_compact);
    assert_eq!(terms, ["^test\\(", "tests/example.mjs"]);
    assert_eq!(
        cli.raw_args,
        [
            "cx",
            "rg",
            "--no-compact",
            "-n",
            "^test\\(",
            "tests/example.mjs"
        ]
    );
}

#[test]
fn explicit_rg_regexp_can_search_for_no_compact_literal() {
    let cli =
        try_parse_from_cx_args(["cx", "--", "rg", "-F", "-e", "--no-compact", "src"]).unwrap();
    let Command::Grep {
        fixed_strings,
        no_compact,
        patterns,
        terms,
        ..
    } = cli.command
    else {
        panic!("expected rg wrapper");
    };
    assert!(fixed_strings);
    assert!(!no_compact);
    assert_eq!(patterns, ["--no-compact"]);
    assert_eq!(terms, ["src"]);
}

#[test]
fn explicit_separator_passthroughs_native_rg_max_count_flags() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            for case in [
                vec!["cx", "--", "rg", "--max-count", "20", "-n", "needle", "src"],
                vec!["cx", "--", "rg", "--max-count=20", "-n", "needle", "src"],
            ] {
                let cli = try_parse_from_cx_args(case.iter().copied()).unwrap();
                let Command::Passthrough { args } = cli.command else {
                    panic!("expected native rg passthrough for {case:?}");
                };
                assert_eq!(args, case[2..]);
            }
        },
    );
}

#[test]
fn explicit_separator_passthroughs_native_rg_regex_engine_flags() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            for case in [
                vec![
                    "cx",
                    "--",
                    "rg",
                    "-o",
                    "--pcre2",
                    ".{0,220}translateY\\(50px\\).{0,260}",
                    "steamui.css",
                ],
                vec![
                    "cx",
                    "--",
                    "rg",
                    "--engine=pcre2",
                    "translateY\\(50px\\)",
                    "steamui.css",
                ],
            ] {
                let cli = try_parse_from_cx_args(case.iter().copied()).unwrap();
                let Command::Passthrough { args } = cli.command else {
                    panic!("expected native rg passthrough for {case:?}");
                };
                assert_eq!(args, case[2..]);
                assert_eq!(cli.raw_args, case);
            }
        },
    );
}

#[test]
fn explicit_separator_passthroughs_native_rg_no_filename_flag() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let case = [
                "cx",
                "--",
                "rg",
                "-o",
                "--no-filename",
                "\"preview/[A-Za-z0-9_./-]+\"",
                "packages/intelligence-ui/src/chat",
            ];
            let cli = try_parse_from_cx_args(case).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected native rg passthrough");
            };
            assert_eq!(args, case[2..]);
            assert_eq!(cli.raw_args, case);
        },
    );
}

#[test]
fn explicit_separator_dispatches_node_check_when_parse_is_clear() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli =
                try_parse_from_cx_args(["cx", "--", "node", "--check", "script.mjs"]).unwrap();
            let Command::Node { args } = cli.command else {
                panic!("expected node wrapper");
            };
            assert_eq!(args, ["--check", "script.mjs"]);
            assert_eq!(cli.raw_args, ["cx", "node", "--check", "script.mjs"]);

            let loader_cli = try_parse_from_cx_args([
                "cx",
                "--",
                "node",
                "--experimental-loader",
                "./jsx_loader.mjs",
                "--check",
                "widget.jsx",
            ])
            .unwrap();
            let Command::Node { args } = loader_cli.command else {
                panic!("expected node wrapper");
            };
            assert_eq!(
                args,
                [
                    "--experimental-loader",
                    "./jsx_loader.mjs",
                    "--check",
                    "widget.jsx"
                ]
            );
            assert_eq!(
                loader_cli.raw_args,
                [
                    "cx",
                    "node",
                    "--experimental-loader",
                    "./jsx_loader.mjs",
                    "--check",
                    "widget.jsx"
                ]
            );
        },
    );
}

#[test]
fn explicit_separator_dispatches_node_runtime_modes_when_parse_is_clear() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let run_cli =
                try_parse_from_cx_args(["cx", "--", "node", "run", "script.mjs"]).unwrap();
            let Command::Node { args } = run_cli.command else {
                panic!("expected node wrapper");
            };
            assert_eq!(args, ["run", "script.mjs"]);
            assert_eq!(run_cli.raw_args, ["cx", "node", "run", "script.mjs"]);

            let test_cli =
                try_parse_from_cx_args(["cx", "--", "node", "test", "script.test.mjs"]).unwrap();
            let Command::Node { args } = test_cli.command else {
                panic!("expected node wrapper");
            };
            assert_eq!(args, ["test", "script.test.mjs"]);
            assert_eq!(test_cli.raw_args, ["cx", "node", "test", "script.test.mjs"]);
        },
    );
}

#[test]
fn explicit_separator_dispatches_node_input_type_when_parse_is_clear() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args(["cx", "--", "node", "--input-type=module"]).unwrap();
            let Command::Node { args } = cli.command else {
                panic!("expected node wrapper");
            };
            assert_eq!(args, ["--input-type=module"]);
            assert_eq!(cli.raw_args, ["cx", "node", "--input-type=module"]);
        },
    );
}

#[test]
fn explicit_separator_dispatches_node_stdin_runtime_when_parse_is_clear() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args(["cx", "--", "node"]).unwrap();
            let Command::Node { args } = cli.command else {
                panic!("expected node wrapper");
            };
            assert!(args.is_empty());
            assert_eq!(cli.raw_args, ["cx", "node"]);
        },
    );
}

#[test]
fn explicit_separator_dispatches_supported_find_when_parse_is_clear() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", None),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = try_parse_from_cx_args([
                "cx",
                "--",
                "find",
                "src",
                "-maxdepth",
                "2",
                "-type",
                "f",
                "-name",
                "*.rs",
                "-print",
            ])
            .unwrap();
            let Command::Find { args } = cli.command else {
                panic!("expected find wrapper");
            };
            assert_eq!(
                args,
                [
                    "src",
                    "-maxdepth",
                    "2",
                    "-type",
                    "f",
                    "-name",
                    "*.rs",
                    "-print"
                ]
            );
            assert_eq!(
                cli.raw_args,
                [
                    "cx",
                    "find",
                    "src",
                    "-maxdepth",
                    "2",
                    "-type",
                    "f",
                    "-name",
                    "*.rs",
                    "-print"
                ]
            );
        },
    );
}

#[test]
fn explicit_separator_uses_passthrough_for_find_boolean_expressions() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            for case in [
                &[
                    "cx", "--", "find", "build", "-type", "f", "(", "-name", "one", "-o", "-name",
                    "two", ")", "-print",
                ][..],
                &[
                    "cx", "--", "find", "build", "-type", "f", "-a", "-name", "one",
                ][..],
            ] {
                let cli = try_parse_from_cx_args(case.iter().copied()).unwrap();
                let Command::Passthrough { args } = cli.command else {
                    panic!("expected native find passthrough for {case:?}");
                };
                assert_eq!(args, case[2..]);
                assert_eq!(cli.raw_args, case);
            }
        },
    );
}

#[test]
fn explicit_separator_uses_passthrough_for_node_runtime_invocation() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli = try_parse_from_cx_args(["cx", "--", "node", "script.mjs"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(args, ["node", "script.mjs"]);
        },
    );
}

#[test]
fn explicit_separator_preserves_trailing_check_as_a_node_script_argument() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli =
                try_parse_from_cx_args(["cx", "--", "node", "script.mjs", "--check"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected native node passthrough");
            };
            assert_eq!(args, ["node", "script.mjs", "--check"]);
            assert_eq!(cli.raw_args, ["cx", "--", "node", "script.mjs", "--check"]);
        },
    );
}

#[test]
fn explicit_separator_uses_passthrough_for_find_exec() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli = try_parse_from_cx_args([
                "cx",
                "--",
                "find",
                "captures",
                "-maxdepth",
                "1",
                "-type",
                "f",
                "-name",
                "*.json",
                "-print",
                "-exec",
                "sed",
                "-n",
                "1,220p",
                "{}",
                ";",
            ])
            .unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(
                args,
                [
                    "find",
                    "captures",
                    "-maxdepth",
                    "1",
                    "-type",
                    "f",
                    "-name",
                    "*.json",
                    "-print",
                    "-exec",
                    "sed",
                    "-n",
                    "1,220p",
                    "{}",
                    ";"
                ]
            );
        },
    );
}

#[test]
fn explicit_separator_uses_passthrough_for_parser_risky_grep() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli =
                try_parse_from_cx_args(["cx", "--", "grep", "-RInE", "needle", "src"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(args, ["grep", "-RInE", "needle", "src"]);
        },
    );
}

#[test]
fn explicit_separator_does_not_passthrough_conflicting_read_windows() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let error = try_parse_from_cx_args([
                "cx",
                "--",
                "read",
                "--head",
                "5",
                "--tail",
                "5",
                "fixture.rs",
            ])
            .unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        },
    );
}

#[test]
fn explicit_separator_does_not_passthrough_unknown_read_flags() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let error =
                try_parse_from_cx_args(["cx", "--", "read", "--definitely-not-real", "fixture.rs"])
                    .unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        },
    );
}

#[test]
fn explicit_separator_uses_passthrough_for_unsupported_supported_root_subcommand() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli =
                try_parse_from_cx_args(["cx", "--", "git", "branch", "--show-current"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(args, ["git", "branch", "--show-current"]);
        },
    );
}

#[test]
fn unsupported_passthrough_accepts_unsupported_subcommand_when_enabled() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            let cli = try_parse_from_cx_args(["cx", "git", "branch", "--show-current"]).unwrap();
            let Command::Passthrough { args } = cli.command else {
                panic!("expected passthrough command");
            };
            assert_eq!(args, ["git", "branch", "--show-current"]);
        },
    );
}

#[test]
fn unsupported_passthrough_does_not_mask_supported_command_flag_errors() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            assert!(try_parse_from_cx_args(["cx", "read", "--definitely-not-real"]).is_err());
        },
    );
}
