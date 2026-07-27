use super::execute;
use super::read::{build_read_options, parse_read_range, ReadDispatchArgs};
use super::telemetry::{command_label, invocation_command_label};
use crate::cli::{Cli, ReadCliMode};
use crate::commands;
use crate::support::insights::{self, InvocationRecord, TextMetrics};
use std::fs;
use std::path::Path;

use clap::Parser;

#[test]
fn smart_read_rejects_window_and_format_flags() {
    let error = build_read_options(ReadDispatchArgs {
        head: Some(10),
        tail: None,
        range_spec: None,
        full: false,
        line_numbers: false,
        raw: false,
        mode: Some(ReadCliMode::Smart),
        smart: false,
        max_lines: None,
        no_auto_aggressive: false,
    })
    .unwrap_err();
    assert!(error.to_string().contains("conflicts"));
}

#[test]
fn smart_read_rejects_range() {
    let error = build_read_options(ReadDispatchArgs {
        head: None,
        tail: None,
        range_spec: Some("4:8"),
        full: false,
        line_numbers: false,
        raw: false,
        mode: None,
        smart: true,
        max_lines: None,
        no_auto_aggressive: false,
    })
    .unwrap_err();
    assert!(error.to_string().contains("conflicts"));
}

#[test]
fn range_parsing_supports_open_ended_ranges() {
    let parsed = parse_read_range(Some("12:")).unwrap().unwrap();
    assert_eq!(
        parsed,
        commands::read::ReadRange {
            start: Some(12),
            end: None,
        }
    );
}

#[test]
fn command_label_breaks_down_subcommands() {
    let git_diff = Cli::try_parse_from(["cx", "git", "diff"]).unwrap();
    assert_eq!(command_label(&git_diff.command), "git diff");

    let docker_logs = Cli::try_parse_from(["cx", "docker", "logs", "web"]).unwrap();
    assert_eq!(command_label(&docker_logs.command), "docker logs");

    let grep_files = Cli::try_parse_from(["cx", "grep", "--files", "src"]).unwrap();
    assert_eq!(command_label(&grep_files.command), "grep files");

    let insights = Cli::try_parse_from(["cx", "insights", "summary"]).unwrap();
    assert_eq!(command_label(&insights.command), "insights");
}

#[test]
fn invocation_command_label_breaks_search_down_by_alias_and_dialect() {
    let cases = [
        (vec!["cx", "grep", "needle", "src"], "grep basic"),
        (
            vec!["cx", "grep", "-E", "needle|term", "src"],
            "grep extended",
        ),
        (vec!["cx", "grep", "-F", "needle", "src"], "grep fixed"),
        (vec!["cx", "rg", "needle|term", "src"], "rg extended"),
        (vec!["cx", "rg", "-F", "needle", "src"], "rg fixed"),
        (vec!["cx", "--", "rg", "--files", "src"], "rg files"),
    ];

    for (args, expected) in cases {
        let cli = crate::cli::parse_from_cx_args(args);
        assert_eq!(invocation_command_label(&cli), expected);
    }
}

#[test]
fn passthrough_telemetry_uses_canonical_process_and_family_identity() {
    crate::support::test_support::with_env_vars(
        &[("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1"))],
        || {
            for (args, process, family) in [
                (vec!["cx", "--", "npm", "run", "build"], "npm", "npm build"),
                (
                    vec!["cx", "--", "dotnet", "test", "app.sln"],
                    "dotnet",
                    "dotnet test",
                ),
                (vec!["cx", "--", "jq", ".items"], "jq", "passthrough jq"),
            ] {
                let cli = crate::cli::parse_from_cx_args(args);
                assert_eq!(invocation_command_label(&cli), family);
                assert_eq!(
                    super::telemetry::command_root(&cli.command, family),
                    process
                );
            }
        },
    );
}

#[cfg(unix)]
#[test]
fn execute_records_search_alias_dialect_and_backend_route() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "rg",
        "#!/bin/sh\nif [ \"$1\" = \"--files\" ]; then printf 'src/main.rs\\n'; else printf 'src/main.rs:7:needle here\\n'; fi\n",
    );

    let path = bin.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let commands = [
                vec!["cx", "grep", "needle", "src"],
                vec!["cx", "grep", "-E", "needle|term", "src"],
                vec!["cx", "grep", "-F", "needle", "src"],
                vec!["cx", "rg", "needle|term", "src"],
                vec!["cx", "rg", "-F", "needle", "src"],
                vec!["cx", "--", "rg", "--files", "src"],
            ];
            for args in commands {
                let cli = crate::cli::parse_from_cx_args(args);
                assert_eq!(execute(&cli).unwrap(), 0);
            }
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let mut statement = connection
        .prepare("SELECT process, command_family, source FROM command_invocations ORDER BY id ASC")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();

    assert_eq!(
        rows.iter()
            .map(|row| (row.0.as_str(), row.1.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("grep", "grep basic"),
            ("grep", "grep extended"),
            ("grep", "grep fixed"),
            ("rg", "rg extended"),
            ("rg", "rg fixed"),
            ("rg", "rg files"),
        ]
    );
    assert!(rows
        .iter()
        .all(|row| row.2.contains("backend=rg route=preferred")));
    assert!(rows[0].2.contains("dialect=basic mode=matches"));
    assert!(rows[1].2.contains("dialect=extended mode=matches"));
    assert!(rows[2].2.contains("dialect=fixed mode=matches"));
    assert!(rows[5].2.contains("dialect=none mode=files"));
}

#[test]
fn execute_records_insights_to_configured_sqlite_database() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.txt");
    std::fs::write(&source, "one\ntwo\nthree\nfour\n").unwrap();
    let db_path = temp.path().join("insights.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    let source_string = source.to_string_lossy().to_string();

    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli = Cli::try_parse_from(["cx", "read", "--head", "1", &source_string]).unwrap();
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let invocation_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM command_invocations", [], |row| {
            row.get(0)
        })
        .unwrap();
    let saved_lines: i64 = connection
        .query_row(
            "SELECT saved_lines FROM command_totals WHERE command = 'read'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(invocation_count, 1);
    assert_eq!(saved_lines, 3);
}

#[test]
fn execute_records_readable_invocation_identity() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.txt");
    std::fs::write(&source, "one\ntwo\nthree\nfour\n").unwrap();
    let db_path = temp.path().join("insights.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    let source_string = source.to_string_lossy().to_string();

    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli =
                crate::cli::parse_from_cx_args(["cx", "read", "--range", "2:3", &source_string]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "SELECT process, command_family, command, source, argv_json FROM command_invocations",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "read");
    assert_eq!(row.1, "read");
    assert_eq!(row.2, format!("read --range 2:3 {source_string}"));
    assert_eq!(row.3, source_string);
    assert!(row.4.contains("--range"));
}

#[test]
fn execute_records_explicit_shell_without_auto_separator() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("insights.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();

    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let cli =
                crate::cli::parse_from_cx_args(["cx", "--", "bash", "-lc", "printf shell-ok"]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "SELECT process, command_family, command, argv_json FROM command_invocations",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "sh");
    assert_eq!(row.1, "sh");
    assert_eq!(row.2, "bash -lc 'printf shell-ok'");
    assert!(row.3.contains(r#""--""#));
}

#[test]
fn execute_records_shape_without_full_command_text() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("source.txt");
    std::fs::write(&source, "one\ntwo\nthree\nfour\n").unwrap();
    let source_string = source.to_string_lossy().to_string();

    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            insights::set_insight_setting("record_invocations", "true").unwrap();
            let cli =
                crate::cli::parse_from_cx_args(["cx", "read", "--range", "2:3", &source_string]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(home.join(".cx/db.sqlite")).unwrap();
    let row = connection
        .query_row(
            "
                SELECT command, argv_json, command_shape, command_shape_hash
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "read");
    assert_eq!(row.1, "[]");
    assert_eq!(row.2, "read --range '<range>' '<path:txt>'");
    assert!(!row.2.contains(&source_string));
    assert!(!row.3.is_empty());
}

#[test]
fn execute_records_unsupported_passthrough_identity() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "demo-tool",
        "#!/bin/sh\nprintf 'demo:%s\\n' \"$1\"\n",
    );

    let path = bin.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args(["cx", "demo-tool", "alpha"]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "
                SELECT process, command_family, command, source, argv_json,
                       raw_chars, emitted_chars, saved_chars
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, u64>(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "demo-tool");
    assert_eq!(row.1, "passthrough demo-tool");
    assert_eq!(row.2, "demo-tool alpha");
    assert_eq!(row.3, "passthrough:demo-tool");
    assert!(row.4.contains("demo-tool"));
    assert_eq!(row.5, row.6);
    assert_eq!(row.7, 0);
}

#[test]
fn execute_normalizes_path_invoked_passthrough_identity_and_shape() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("sample-suite-tests");
    crate::support::test_support::write_executable(
        &bin,
        "sample-suite-tests",
        "#!/bin/sh\nprintf 'planner fixture ok\\n'\n",
    );

    let executable_string = executable.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args([
                "cx",
                "--",
                executable_string.as_str(),
                "--gtest_filter=Planner.*",
            ]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "
                SELECT process, command_family, command_shape, command_shape_hash
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "sample-suite-tests");
    assert_eq!(row.1, "passthrough sample-suite-tests");
    assert_eq!(row.2, "sample-suite-tests '--gtest_filter=<glob>'");
    assert!(!row.2.contains(temp.path().to_string_lossy().as_ref()));
    assert!(!row.3.is_empty());
}

#[test]
fn execute_redacts_secret_like_passthrough_program_across_identity_fields() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    let secret_name = "SK-PROJ-ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let executable = bin.join(secret_name);
    crate::support::test_support::write_executable(
        &bin,
        secret_name,
        "#!/bin/sh\nprintf 'secret fixture executed\\n'\n",
    );

    let executable_string = executable.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args([
                "cx",
                "--",
                executable_string.as_str(),
                "--version",
            ]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "
                SELECT process, command_family, command, source, argv_json, command_shape
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "unknown");
    assert_eq!(row.1, "unknown");
    assert_eq!(row.3, "passthrough:unknown");
    for value in [&row.2, &row.3, &row.4, &row.5] {
        assert!(
            !value.contains(secret_name),
            "secret leaked through `{value}`"
        );
    }
    assert_eq!(row.5, "'<redacted>' --version");
}

#[test]
fn execute_explicit_separator_routes_supported_git_diff_to_wrapper() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "git",
            "#!/bin/sh\nif [ \"$1\" = diff ] && [ \"$2\" = --stat ]; then printf 'src/lib.rs | 800 +++++++++++++++++++++++++++++++++++++++++\\n'; exit 0; fi\nif [ \"$1\" = diff ]; then printf 'diff --git a/src/lib.rs b/src/lib.rs\\n@@ -1,3 +1,800 @@\\n'; i=1; while [ \"$i\" -le 800 ]; do printf '+line %03d\\n' \"$i\"; i=$((i + 1)); done; exit 0; fi\nprintf 'unexpected git args: %s %s\\n' \"$1\" \"$2\" >&2\nexit 2\n",
        );

    let path = bin.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args(["cx", "--", "git", "diff"]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "
                SELECT process, command_family, command, source, argv_json, saved_chars
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, u64>(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "git");
    assert_eq!(row.1, "git diff");
    assert_eq!(row.2, "git diff");
    assert_eq!(row.3, "git diff");
    assert!(!row.4.contains("\"--\""));
    assert!(row.5 > 0);
}

#[test]
fn execute_explicit_separator_passthroughs_unsupported_git_subcommand() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "git",
        "#!/bin/sh\nif [ \"$1\" = branch ]; then printf 'main\\n'; exit 0; fi\nexit 2\n",
    );

    let path = bin.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli =
                crate::cli::parse_from_cx_args(["cx", "--", "git", "branch", "--show-current"]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "
                SELECT process, command_family, command, source, saved_chars
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u64>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "git");
    assert_eq!(row.1, "passthrough git");
    assert_eq!(row.2, "git branch --show-current");
    assert_eq!(row.3, "passthrough:git");
    assert_eq!(row.4, 0);
}

#[test]
fn execute_explicit_separator_passthroughs_node_runtime_invocation() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "node",
        "#!/bin/sh\nprintf 'runtime:%s\\n' \"$1\"\n",
    );

    let path = bin.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args(["cx", "--", "node", "script.mjs"]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let row = connection
        .query_row(
            "
                SELECT process, command_family, command, source, saved_chars
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u64>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "node");
    assert_eq!(row.1, "node run");
    assert_eq!(row.2, "node script.mjs");
    assert_eq!(row.3, "passthrough:node");
    assert_eq!(row.4, 0);
}

#[test]
fn execute_records_passthrough_opportunity_estimate_for_large_text_output() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "long-tool",
            "#!/bin/sh\ni=1\nwhile [ \"$i\" -le 80 ]; do printf 'row-%03d\\n' \"$i\"; i=$((i + 1)); done\n",
        );

    let path = bin.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args(["cx", "long-tool"]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let saved_chars: u64 = connection
            .query_row(
                "SELECT saved_chars FROM command_invocations WHERE command_family = 'passthrough long-tool'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(saved_chars, 0);

    let opportunity = connection
        .query_row(
            "
                SELECT command_family, potential_saved_lines, potential_saved_tokens, strategy,
                       confidence
                FROM command_opportunities
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(opportunity.0, "passthrough long-tool");
    assert!(opportunity.1 > 0);
    assert!(opportunity.2 > 0);
    assert_eq!(opportunity.3, "generic-head-tail-12-28");
    assert_eq!(opportunity.4, "low");
}

#[test]
fn execute_records_passthrough_opportunity_for_generated_one_line_output() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "json-tool",
        "#!/bin/sh\nprintf '{\"status\":\"ok\",\"payload\":\"'\ni=0; while [ \"$i\" -lt 20000 ]; do printf A; i=$((i + 1)); done\nprintf '\",\"tail\":\"done\"}\\n'\n",
    );

    let path = bin.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args(["cx", "json-tool"]);
            assert_eq!(execute(&cli).unwrap(), 0);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let invocation = connection
        .query_row(
            "SELECT raw_chars, emitted_chars, saved_chars FROM command_invocations",
            [],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(invocation.0, invocation.1);
    assert_eq!(invocation.2, 0);

    let opportunity = connection
        .query_row(
            "
                SELECT command_family, potential_saved_chars, potential_saved_tokens,
                       potential_saved_lines, strategy, confidence
                FROM command_opportunities
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(opportunity.0, "passthrough json-tool");
    assert!(opportunity.1 > 18_000);
    assert!(opportunity.2 > 1_000);
    assert_eq!(opportunity.3, 0);
    assert_eq!(opportunity.4, "generic-generated-line-1200");
    assert_eq!(opportunity.5, "high");
}

#[test]
fn execute_insights_queries_do_not_record_themselves() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("insights.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();

    crate::support::test_support::with_env_vars(
        &[
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let raw = insights::OutputObservation::from_text("read source", "a\nb\nc\n");
            insights::record_invocation(&InvocationRecord {
                command: "read",
                exit_code: 0,
                raw: Some(&raw),
                emitted: TextMetrics::from_text("a\n"),
            })
            .unwrap();
            let commands = [
                vec!["cx", "insights", "summary"],
                vec!["cx", "insights", "recommend", "--limit", "4"],
                vec!["cx", "insights", "opportunities", "--limit", "4"],
                vec!["cx", "insights", "failures", "--limit", "4"],
                vec![
                    "cx", "insights", "top", "--sort", "failures", "--limit", "3",
                ],
                vec!["cx", "insights", "recent", "--limit", "3"],
                vec!["cx", "insights", "report", "--limit", "3"],
                vec!["cx", "insights", "reports", "--limit", "4"],
                vec!["cx", "insights", "dashboard", "--limit", "4"],
                vec!["cx", "insights", "audit", "--limit", "4"],
                vec![
                    "cx", "insights", "audit", "--format", "json", "--limit", "4",
                ],
                vec!["cx", "insights", "settings"],
                vec![
                    "cx", "insights", "export", "--format", "json", "--limit", "3",
                ],
            ];

            for command in commands {
                let cli = Cli::try_parse_from(command).unwrap();
                assert_eq!(execute(&cli).unwrap(), 0);
            }
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let invocation_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM command_invocations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(invocation_count, 1);
}

#[cfg(unix)]
#[test]
fn execute_records_failure_detail_with_cx_and_raw_responses() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "rg",
        "#!/bin/sh\nprintf 'regex parse error: bad pattern\\n' >&2\nexit 2\n",
    );

    let path = bin.to_string_lossy().to_string();
    let home_path = home.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args([
                "cx",
                "rg",
                "(",
                "sk-proj-abcdefghijklmnopqrstuvwxyz",
                "src",
            ]);
            assert_eq!(execute(&cli).unwrap(), 2);
        },
    );

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let invocation = connection
        .query_row(
            "
                SELECT process, command_family, command, argv_json
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(invocation.0, "rg");
    assert_eq!(invocation.1, "rg extended");
    assert!(invocation.2.starts_with("rg "));
    assert!(invocation.2.contains("[REDACTED]"));
    assert!(!invocation.2.contains("sk-proj-abcdefghijklmnopqrstuvwxyz"));
    assert!(invocation.3.contains("[REDACTED]"));
    assert!(!invocation.3.contains("sk-proj-abcdefghijklmnopqrstuvwxyz"));

    let detail = connection
        .query_row(
            "
                SELECT command_family, command_line, exit_code,
                       cx_response, raw_source, raw_response
                FROM command_failures
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(detail.0, "rg extended");
    assert_eq!(detail.1, "cx rg '(' '[REDACTED]' src");
    assert_eq!(detail.2, 2);
    assert!(detail.3.contains("[full output:"));
    assert!(detail.3.contains("regex parse error"));
    assert_eq!(
        detail.4,
        "search backend=rg route=preferred dialect=extended mode=matches result=error hint=none"
    );
    assert!(detail.5.contains("regex parse error: bad pattern"));
}

#[cfg(unix)]
#[test]
fn execute_records_passthrough_failure_artifact_and_native_response() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let db_path = temp.path().join("insights.sqlite");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "demo-fail",
        "#!/bin/sh\nprintf 'native partial stdout\\n'\nprintf 'native stderr\\n' >&2\nexit 7\n",
    );

    let path = bin.to_string_lossy().to_string();
    let home_path = home.to_string_lossy().to_string();
    let db_path_string = db_path.to_string_lossy().to_string();
    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_path.as_str())),
            ("CX_INSIGHTS_DB_PATH", Some(db_path_string.as_str())),
            ("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", Some("1")),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let cli = crate::cli::parse_from_cx_args([
                "cx",
                "--",
                "demo-fail",
                "sk-proj-abcdefghijklmnopqrstuvwxyz",
            ]);
            assert_eq!(execute(&cli).unwrap(), 7);
        },
    );

    assert_passthrough_failure_artifact(&home);
    assert_passthrough_failure_telemetry(&db_path);
}

fn assert_passthrough_failure_artifact(home: &Path) {
    let artifact_dir = home.join(".cx/cache/failures/passthrough");
    let artifacts = fs::read_dir(&artifact_dir)
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    let artifact = fs::read_to_string(artifacts[0].path()).unwrap();
    assert!(artifact.contains("native partial stdout"));
    assert!(artifact.contains("native stderr"));
    assert!(!artifact.contains("sk-proj-abcdefghijklmnopqrstuvwxyz"));
}

fn assert_passthrough_failure_telemetry(db_path: &Path) {
    let connection = rusqlite::Connection::open(db_path).unwrap();
    let invocation = connection
        .query_row(
            "
                SELECT process, command_family, command, exit_code,
                       raw_chars, emitted_chars, expanded_chars
                FROM command_invocations
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(invocation.0, "demo-fail");
    assert_eq!(invocation.1, "passthrough demo-fail");
    assert!(invocation.2.contains("demo-fail"));
    assert!(invocation.2.contains("[REDACTED]"));
    assert!(!invocation.2.contains("sk-proj-abcdefghijklmnopqrstuvwxyz"));
    assert_eq!(invocation.3, 7);
    assert!(invocation.5 > invocation.4);
    assert_eq!(invocation.6, invocation.5 - invocation.4);

    let detail = connection
        .query_row(
            "
                SELECT command_family, command_line, exit_code,
                       cx_response, raw_source, raw_response
                FROM command_failures
                ",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(detail.0, "passthrough demo-fail");
    assert!(detail.1.contains("demo-fail"));
    assert!(detail.1.contains("[REDACTED]"));
    assert!(!detail.1.contains("sk-proj-abcdefghijklmnopqrstuvwxyz"));
    assert_eq!(detail.2, 7);
    assert!(detail.3.contains("native partial stdout"));
    assert!(detail.3.contains("native stderr"));
    assert!(detail
        .3
        .contains("[full output: ~/.cx/cache/failures/passthrough/"));
    assert_eq!(detail.4, "passthrough:demo-fail");
    assert!(detail.5.contains("native partial stdout"));
    assert!(detail.5.contains("native stderr"));
    assert!(!detail.5.contains("[full output:"));
}
