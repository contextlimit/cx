#![cfg(unix)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use clap::Parser;
use cx::cli::Cli;
use cx::commands;
use cx::dispatch;
use tempfile::tempdir;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

const BAD_JQ_PRECEDENCE_FILTER: &str = r#"[.[] | select(.status=="completed")] | length, [.[] | select(.status=="in_progress")] | map(.id) | join(",")"#;
const REPAIRED_JQ_PRECEDENCE_FILTER: &str = r#"([.[] | select(.status=="completed")] | length), ([.[] | select(.status=="in_progress")] | map(.id) | join(","))"#;

fn fake_jq_precedence_script() -> String {
    format!(
        r#"#!/bin/sh
filter="$1"
if [ "$1" = "-r" ]; then
  filter="$2"
fi
repaired='{REPAIRED_JQ_PRECEDENCE_FILTER}'
if [ "$filter" = "$repaired" ]; then
  printf '10\nOB-02\n'
  exit 0
fi
printf 'jq: error (at ledger.json:4920): Cannot iterate over number (10)\n' >&2
exit 5
"#
    )
}

fn assert_cli_shapes_parse(cases: &[&[&str]]) {
    for case in cases {
        assert!(
            Cli::try_parse_from(*case).is_ok(),
            "recent command shape failed to parse: {case:?}"
        );
    }
}

#[test]
fn recent_core_cli_shapes_still_parse() {
    assert_cli_shapes_parse(&[
        &["cx", "read", "--range", "1:220", "fixture.rs"],
        &["cx", "read", "--head", "70", "fixture.rs"],
        &["cx", "read", "--raw", "--head", "40", "fixture.rs"],
        &["cx", "read", "--raw", "--full", "fixture.rs"],
        &["cx", "cat", "fixture.rs"],
        &["cx", "head", "-n", "70", "fixture.rs"],
        &["cx", "tail", "-n", "+40", "fixture.rs"],
        &["cx", "sed", "-n", "1,220p", "fixture.rs"],
        &["cx", "nl", "-ba", "fixture.rs"],
        &["cx", "grep", "-n", "needle", "src"],
        &["cx", "grep", "-e", "--generate", "src"],
        &[
            "cx",
            "grep",
            "-F",
            "-e",
            "data-menu-open",
            "-e",
            "onClick",
            "--glob",
            "*.jsx",
            "--max-results",
            "5",
            "src",
        ],
        &["cx", "grep", "-E", "foo|bar|target_sources\\(", "src"],
        &[
            "cx",
            "rg",
            "-F",
            "-e",
            "dockview",
            "-e",
            "key={`dockview",
            "src",
        ],
        &[
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
        ],
        &[
            "cx",
            "find",
            ".",
            "-maxdepth",
            "4",
            "-type",
            "f",
            "-name",
            "*.rs",
            "-o",
            "-iname",
            "*.mjs",
        ],
        &["cx", "find", ".", "-type", "f", "-perm", "-111"],
        &["cx", "sh", "-lc", "printf ok"],
        &[
            "cx",
            "git",
            "conflict-diff",
            "--stat",
            "--stage",
            "2:3",
            "packages/chat.js",
        ],
    ]);
}

#[test]
fn recent_process_cli_shapes_still_parse() {
    assert_cli_shapes_parse(&[
        &["cx", "node"],
        &["cx", "node", "--check", "a.mjs", "b.jsx", "c.js"],
        &["cx", "node", "--input-type=module"],
        &["cx", "node", "run", "app/tests/example.mjs"],
        &["cx", "node", "test", "app/tests/example.test.mjs"],
        &["cx", "ps", "-axo", "pid,ppid,etime,command"],
        &["cx", "git", "status", "--short"],
        &["cx", "git", "diff", "--stat"],
        &["cx", "git", "diff", "--name-only", "--diff-filter=U"],
        &["cx", "git", "diff", "--", "src/lib.rs"],
        &["cx", "diff", "--", "src/lib.rs"],
        &[
            "cx",
            "git",
            "show",
            "--no-compact",
            "--no-ext-diff",
            "--no-color",
            "--format=medium",
            "--stat",
            "--patch",
            "HEAD",
        ],
        &["cx", "git", "log", "-n", "8", "--oneline"],
        &["cx", "git", "log", "-5", "--oneline"],
        &[
            "cx",
            "git",
            "log",
            "--oneline",
            "--decorate",
            "--graph",
            "--max-count=20",
            "--all",
        ],
        &[
            "cx",
            "cmake",
            "build",
            "build-web",
            "--target",
            "sample-ui",
            "-j",
            "8",
        ],
        &["cx", "cargo", "test"],
        &[
            "cx",
            "tsc",
            "--noEmit",
            "--project",
            "targets/web_browser/tsconfig.json",
        ],
        &[
            "cx",
            "ctest",
            "--test-dir",
            "build-web",
            "-R",
            "sample-ui-dockview-perf-mouse-e2e",
            "--output-on-failure",
        ],
        &["cx", "ctest", "-N"],
        &["cx", "ctest", "--test-dir", "build-web", "-N"],
        &[
            "cx",
            "ctest",
            "--test-dir",
            "build-web",
            "-N",
            "-R",
            "sample-ui-catalog-model-test",
        ],
    ]);
}

#[test]
fn recent_insights_cli_shapes_still_parse() {
    assert_cli_shapes_parse(&[
        &["cx", "insights", "summary"],
        &["cx", "insights", "top", "--sort", "tokens", "--limit", "5"],
        &[
            "cx", "insights", "largest", "--sort", "lines", "--limit", "10",
        ],
        &["cx", "insights", "daily", "--limit", "7"],
        &["cx", "insights", "expansions", "--limit", "10"],
        &["cx", "insights", "presentation", "--limit", "4"],
        &[
            "cx",
            "insights",
            "impact",
            "--limit",
            "4",
            "--context-window-tokens",
            "200000",
        ],
        &["cx", "insights", "recommend", "--limit", "5"],
        &["cx", "insights", "opportunities", "--limit", "5"],
        &["cx", "insights", "failures", "--limit", "5"],
        &["cx", "insights", "top", "--sort", "failures"],
        &[
            "cx", "insights", "export", "--format", "json", "--limit", "3",
        ],
        &[
            "cx", "insights", "export", "--format", "csv", "--limit", "3",
        ],
    ]);
}

#[test]
fn recent_cargo_multi_filter_shape_still_parses() {
    assert_cli_shapes_parse(&[&[
        "cx",
        "cargo",
        "test",
        "-p",
        "clob-engine",
        "committed_fee_payout_batch_reconciles_open_obligations_with_manifest",
        "committed_replay_records_fee_obligation_reconciliation",
    ]]);
}

#[test]
fn recent_explicit_separator_auto_shapes_still_parse() {
    for case in [
        &["cx", "--", "git", "diff", "--stat"][..],
        &["cx", "--", "grep", "needle", "src"][..],
        &["cx", "--", "sed", "-n", "1,220p", "fixture.rs"][..],
        &["cx", "--", "head", "-n", "70", "fixture.rs"][..],
        &["cx", "--", "tail", "-n", "+40", "fixture.rs"][..],
        &["cx", "--", "cat", "fixture.rs"][..],
        &["cx", "--", "nl", "-ba", "fixture.rs"][..],
        &["cx", "--", "ps", "-axo", "pid,ppid,etime,command"][..],
        &["cx", "--", "bash", "-lc", "printf ok"][..],
        &["cx", "--", "git", "evidence-diff"][..],
        &["cx", "--", "git", "evidence-diff", "HEAD^..HEAD"][..],
        &[
            "cx",
            "--",
            "git",
            "evidence-diff",
            "HEAD~1..HEAD",
            "--",
            "src/lib.rs",
        ][..],
    ] {
        cx::cli::parse_from_cx_args(case.iter().copied());
    }
}

#[test]
fn explicit_separator_bash_routes_to_shell_without_passthrough_setting() {
    let output = cx_test_command()
        .args(["--", "bash", "-lc", "printf ok"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CX_DISABLE_INSIGHTS", "1")
        .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
}

#[test]
fn explicit_auto_passthrough_preserves_unterminated_native_streams_and_metrics() {
    with_fake_tools(
        &[(
            "byte-tool",
            "#!/bin/sh\nif [ \"$1\" = stderr ]; then printf stderr-byte >&2; else printf stdout-byte; fi\n",
        )],
        |temp| {
            let db_path = temp.join("passthrough-bytes.sqlite");
            for (stream, expected_stdout, expected_stderr) in [
                ("stdout", b"stdout-byte".as_slice(), b"".as_slice()),
                ("stderr", b"".as_slice(), b"stderr-byte".as_slice()),
            ] {
                let output = cx_test_command()
                    .args(["--", "byte-tool", stream])
                    .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                    .env("CX_INSIGHTS_DB_PATH", &db_path)
                    .env_remove("CX_DISABLE_INSIGHTS")
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert_eq!(output.stdout, expected_stdout);
                assert_eq!(output.stderr, expected_stderr);
            }

            let connection = rusqlite::Connection::open(db_path).unwrap();
            let metrics = connection
                .prepare(
                    "SELECT raw_bytes, emitted_bytes FROM command_invocations ORDER BY id",
                )
                .unwrap()
                .query_map([], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert_eq!(metrics, vec![(11, 11), (11, 11)]);
        },
    );
}

#[test]
fn explicit_auto_node_script_trailing_check_preserves_runtime_argv() {
    with_fake_tools(
        &[("node", "#!/bin/sh\nprintf 'runtime argv: %s\\n' \"$*\"\n")],
        |temp| {
            let database = temp.join("node-trailing-check.sqlite");
            let output = cx_test_command()
                .args(["--", "node", "script.mjs", "--check"])
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env("CX_INSIGHTS_DB_PATH", database)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();

            assert!(
                output.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "runtime argv: script.mjs --check\n"
            );
        },
    );
}

#[test]
fn explicit_auto_git_diff_forwards_restored_pathspec_separator() {
    with_fake_tools(
        &[(
            "git",
            "#!/bin/sh\nif [ \"$*\" != 'diff --name-only -- src/lib.rs' ]; then\nprintf 'unexpected git args: %s\\n' \"$*\" >&2\nexit 97\nfi\nprintf 'src/lib.rs\\n'\n",
        )],
        |_| {
            let output = cx_test_command()
                .args(["--", "git", "diff", "--name-only", "--", "src/lib.rs"])
                .env("CX_DISABLE_INSIGHTS", "1")
                .output()
                .unwrap();

            assert!(
                output.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), "src/lib.rs\n");
        },
    );
}

#[test]
fn explicit_auto_rg_max_count_forwards_native_argv() {
    with_fake_tools(
        &[(
            "rg",
            "#!/bin/sh\nif [ \"$*\" != '--max-count 20 -n needle src' ]; then\nprintf 'unexpected rg args: %s\\n' \"$*\" >&2\nexit 97\nfi\nprintf 'src/a.rs:1:needle\\nsrc/b.rs:2:needle\\n'\n",
        )],
        |temp| {
            let database = temp.join("rg-max-count.sqlite");
            let output = cx_test_command()
                .args(["--", "rg", "--max-count", "20", "-n", "needle", "src"])
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env("CX_INSIGHTS_DB_PATH", database)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();

            assert!(
                output.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "src/a.rs:1:needle\nsrc/b.rs:2:needle\n"
            );
        },
    );
}

#[test]
fn explicit_auto_rg_pcre2_forwards_native_argv() {
    with_fake_tools(
        &[(
            "rg",
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_TEST_ARGV_PATH\"\nprintf 'transform:translateY(50px)\\n'\n",
        )],
        |temp| {
            let database = temp.join("rg-pcre2.sqlite");
            let argv_path = temp.join("rg-pcre2-argv.txt");
            let pattern = ".{0,220}translateY\\(50px\\).{0,260}";
            let output = cx_test_command()
                .args(["--", "rg", "-o", "--pcre2", pattern, "steamui.css"])
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env("CX_INSIGHTS_DB_PATH", database)
                .env("CX_TEST_ARGV_PATH", &argv_path)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();

            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "transform:translateY(50px)\n"
            );
            assert_eq!(
                fs::read_to_string(argv_path).unwrap(),
                format!("-o\n--pcre2\n{pattern}\nsteamui.css\n")
            );
        },
    );
}

#[test]
fn explicit_auto_rg_no_filename_forwards_native_argv() {
    with_fake_tools(
        &[(
            "rg",
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_TEST_ARGV_PATH\"\nprintf '\"preview/alpha-beta.json\"\\n'\n",
        )],
        |temp| {
            let database = temp.join("rg-no-filename.sqlite");
            let argv_path = temp.join("rg-no-filename-argv.txt");
            let pattern = "\"preview/[A-Za-z0-9_./-]+\"";
            let output = cx_test_command()
                .args([
                    "--",
                    "rg",
                    "-o",
                    "--no-filename",
                    pattern,
                    "packages/intelligence-ui/src/chat",
                ])
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env("CX_INSIGHTS_DB_PATH", database)
                .env("CX_TEST_ARGV_PATH", &argv_path)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();

            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "\"preview/alpha-beta.json\"\n"
            );
            assert_eq!(
                fs::read_to_string(argv_path).unwrap(),
                format!(
                    "-o\n--no-filename\n{pattern}\npackages/intelligence-ui/src/chat\n"
                )
            );
        },
    );
}

#[test]
fn official_read_like_routes_preserve_native_line_terminators() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("terminators.txt");
    fs::write(&path, b"one\r\ntwo\r\nthree").unwrap();
    let path_text = path.to_string_lossy().to_string();

    for (args, expected) in [
        (
            vec!["--", "read", "--range", "1:2", path_text.as_str()],
            b"one\r\ntwo\r\n".as_slice(),
        ),
        (
            vec!["--", "sed", "-n", "2,$p", path_text.as_str()],
            b"two\r\nthree".as_slice(),
        ),
        (
            vec!["--", "head", "-n", "1", path_text.as_str()],
            b"one\r\n".as_slice(),
        ),
        (
            vec!["--", "tail", "-n", "2", path_text.as_str()],
            b"two\r\nthree".as_slice(),
        ),
    ] {
        let output = cx_test_command()
            .args(args)
            .env("CX_DISABLE_INSIGHTS", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected);
    }
}

#[test]
fn official_find_reports_missing_roots_without_hiding_partial_matches() {
    let temp = tempdir().unwrap();
    let valid = temp.path().join("valid");
    let missing = temp.path().join("missing");
    fs::create_dir_all(&valid).unwrap();
    fs::write(valid.join("proof.txt"), "proof\n").unwrap();

    let output = cx_test_command()
        .args([
            "--",
            "find",
            valid.to_str().unwrap(),
            missing.to_str().unwrap(),
            "-type",
            "f",
        ])
        .env("CX_DISABLE_INSIGHTS", "1")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("proof.txt"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("incomplete traversal"));
    assert!(stderr.contains(missing.to_str().unwrap()));
}

#[test]
fn report_recovers_exact_redacted_failure_evidence_without_general_previews() {
    with_fake_tools(
        &[(
            "fail-tool",
            "#!/bin/sh\nprintf 'native response sk-abcdefghijklmnopqrstuvwxyz\\n'\nexit 7\n",
        )],
        |temp| {
            let database = temp.join("reports.sqlite");
            let home = temp.join("report-home");
            fs::create_dir_all(&home).unwrap();
            let settings = Command::new(env!("CARGO_BIN_EXE_cx"))
                .args([
                    "insights",
                    "settings",
                    "--set",
                    "record_invocations=true",
                    "--set",
                    "record_command_text=true",
                    "--set",
                    "record_command_shape=true",
                    "--set",
                    "record_failures=true",
                    "--set",
                    "record_failure_responses=true",
                    "--set",
                    "record_response_previews=false",
                    "--set",
                    "passthrough_unsupported_commands=true",
                ])
                .env("HOME", &home)
                .env("CX_INSIGHTS_DB_PATH", &database)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();
            assert!(
                settings.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&settings.stderr)
            );

            let failure = Command::new(env!("CARGO_BIN_EXE_cx"))
                .args(["--", "fail-tool", "alpha"])
                .env("HOME", &home)
                .env("CX_INSIGHTS_DB_PATH", &database)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();
            assert_eq!(failure.status.code(), Some(7));

            let report = Command::new(env!("CARGO_BIN_EXE_cx"))
                .args(["report", "cx -- fail-tool alpha"])
                .env("HOME", &home)
                .env("CX_INSIGHTS_DB_PATH", &database)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();
            assert!(
                report.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&report.stderr)
            );
            let report_stdout = String::from_utf8_lossy(&report.stdout);
            assert!(report_stdout.contains("Evidence: exact-command:failure-detail"));
            assert!(report_stdout.contains("CX response: recorded"));
            assert!(report_stdout.contains("Native response: recorded"));

            let connection = rusqlite::Connection::open(database).unwrap();
            let evidence = connection
                .query_row(
                    "SELECT invocation_id, evidence_kind, cx_response, native_response
                     FROM command_report_evidence ORDER BY report_id DESC LIMIT 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Option<u64>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .unwrap();
            let failure_invocation: Option<u64> = connection
                .query_row(
                    "SELECT invocation_id FROM command_failures ORDER BY id DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(evidence.0, failure_invocation);
            assert_eq!(evidence.1, "exact-command:failure-detail");
            assert!(evidence.2.contains("[REDACTED]"));
            assert!(evidence.3.contains("[REDACTED]"));
            assert!(!evidence.2.contains("sk-abcdefghijklmnopqrstuvwxyz"));
            assert!(!evidence.3.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        },
    );
}

#[test]
fn explicit_separator_bash_rejects_literal_escaped_heredoc() {
    let output = cx_test_command()
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg("python3 - <<'PY'\\nprint('alpha')\\nPY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CX_DISABLE_INSIGHTS", "1")
        .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("literal \\n escapes"));
    assert!(stderr.contains("cx sh <<'BASH'"));
}

#[test]
fn explicit_separator_bash_rejects_serialized_multiline_script() {
    let output = cx_test_command()
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg("\\nfor item in alpha beta; do\\n  printf '%s\\n' \"$item\"\\ndone\\n")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CX_DISABLE_INSIGHTS", "1")
        .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("serialized \\n separators"));
    assert!(stderr.contains("may already have expanded variables"));
    assert!(!stderr.contains("command not found"));
}

#[test]
fn explicit_separator_bash_no_compact_preserves_long_stdout() {
    let output = cx_test_command()
        .args([
            "--",
            "bash",
            "--no-compact",
            "-lc",
            "for i in {1..80}; do printf 'line_%02d\\n' \"$i\"; done",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CX_DISABLE_INSIGHTS", "1")
        .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 80);
    assert!(stdout.contains("line_01"));
    assert!(stdout.contains("line_80"));
    assert!(!stdout.contains("lines omitted"));
}

#[test]
fn explicit_separator_bash_no_compact_preserves_stdin_script_stdout() {
    let mut child = cx_test_command()
        .args(["--", "bash", "--no-compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CX_DISABLE_INSIGHTS", "1")
        .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"for i in {1..80}; do printf 'stdin_%02d\\n' \"$i\"; done\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 80);
    assert!(stdout.contains("stdin_01"));
    assert!(stdout.contains("stdin_80"));
    assert!(!stdout.contains("lines omitted"));
}

#[test]
fn explicit_auto_read_conflict_does_not_execute_native_read() {
    with_fake_tools(
        &[("read", "#!/bin/sh\nprintf 'native-read-ran\\n'\nexit 0\n")],
        |_| {
            let output = cx_test_command()
                .args(["--", "read", "--head", "5", "--tail", "5", "fixture.rs"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();

            assert_eq!(output.status.code(), Some(2));
            assert!(String::from_utf8_lossy(&output.stdout).is_empty());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("--head"));
            assert!(stderr.contains("--tail"));
            assert!(!stderr.contains("native-read-ran"));
        },
    );
}

#[test]
fn explicit_passthrough_forwards_piped_stdin() {
    with_fake_tools(
        &[(
            "node",
            r#"#!/bin/sh
if [ "$1" != "-" ]; then
  printf 'unexpected node args: %s\n' "$*" >&2
  exit 9
fi
index=1
while IFS= read -r line || [ -n "$line" ]; do
  printf 'stdin[%s]=%s\n' "$index" "$line"
  index=$((index + 1))
done
"#,
        )],
        |temp| {
            let db_path = temp.join("insights.sqlite");
            let mut child = cx_test_command()
                .args(["--", "node", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env("CX_INSIGHTS_DB_PATH", &db_path)
                .env_remove("CX_DISABLE_INSIGHTS")
                .spawn()
                .unwrap();

            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"console.log('alpha')\nconsole.log('beta')\n")
                .unwrap();

            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("stdin[1]=console.log('alpha')"));
            assert!(stdout.contains("stdin[2]=console.log('beta')"));
            assert!(output.stderr.is_empty());
        },
    );
}

#[test]
fn explicit_auto_node_forwards_piped_stdin_without_passthrough() {
    with_fake_tools(
        &[(
            "node",
            r#"#!/bin/sh
if [ "$#" -ne 0 ]; then
  printf 'unexpected node args: %s\n' "$*" >&2
  exit 9
fi
index=1
while IFS= read -r line || [ -n "$line" ]; do
  printf 'stdin[%s]=%s\n' "$index" "$line"
  index=$((index + 1))
done
"#,
        )],
        |temp| {
            let db_path = temp.join("insights.sqlite");
            let mut child = cx_test_command()
                .args(["--", "node"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("CX_INSIGHTS_DB_PATH", &db_path)
                .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
                .env_remove("CX_DISABLE_INSIGHTS")
                .spawn()
                .unwrap();

            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"console.log('alpha')\nconsole.log('beta')\n")
                .unwrap();

            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("stdin[1]=console.log('alpha')"));
            assert!(stdout.contains("stdin[2]=console.log('beta')"));
            assert!(output.stderr.is_empty());
        },
    );
}

#[test]
fn explicit_auto_node_reports_ambiguous_module_syntax_hint() {
    with_fake_tools(
        &[(
            "node",
            r#"#!/bin/sh
while IFS= read -r _line || [ -n "$_line" ]; do
  :
done
printf '%s\n' "ReferenceError: Cannot determine intended module format because both 'require' and top-level await are present." >&2
printf '%s\n' "  code: 'ERR_AMBIGUOUS_MODULE_SYNTAX'" >&2
exit 1
"#,
        )],
        |temp| {
            let db_path = temp.join("insights.sqlite");
            let mut child = cx_test_command()
                .args(["--", "node"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("CX_INSIGHTS_DB_PATH", &db_path)
                .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
                .env_remove("CX_DISABLE_INSIGHTS")
                .spawn()
                .unwrap();

            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"const fs = require('fs');\nawait Promise.resolve();\n")
                .unwrap();

            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("ERR_AMBIGUOUS_MODULE_SYNTAX"));
            assert!(stderr.contains("CX detected Node's ambiguous stdin module-format error"));
            assert!(stderr.contains("wrap the awaited code in an async function"));
            assert!(stderr.contains("--input-type=module"));
        },
    );
}

#[test]
fn explicit_passthrough_repairs_jq_precedence_filter() {
    let jq = fake_jq_precedence_script();
    with_fake_tools(&[("jq", jq.as_str())], |temp| {
        let db_path = temp.join("insights.sqlite");
        let output = cx_test_command()
            .args(["--", "jq", "-r", BAD_JQ_PRECEDENCE_FILTER, "ledger.json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
            .env("CX_INSIGHTS_DB_PATH", &db_path)
            .env_remove("CX_DISABLE_INSIGHTS")
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "10\nOB-02\n");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("jq comma branches"));
        assert!(stderr.contains(REPAIRED_JQ_PRECEDENCE_FILTER));

        let connection = rusqlite::Connection::open(&db_path).unwrap();
        let row = connection
            .query_row(
                "SELECT rule_id, action, original_exit_code, final_exit_code FROM command_repairs",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, i32>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "jq.comma_pipe_precedence");
        assert_eq!(row.1, "auto_retry_success");
        assert_eq!(row.2, 5);
        assert_eq!(row.3, 0);
    });
}

#[test]
fn shell_wrapper_hints_jq_precedence_filter_without_rerunning_shell() {
    let jq = fake_jq_precedence_script();
    with_fake_tools(
        &[
            (
                "bash",
                "#!/bin/sh\nif [ \"$1\" = \"-lc\" ]; then shift; /bin/sh -c \"$1\"; else /bin/cat; fi\n",
            ),
            ("jq", jq.as_str()),
        ],
        |_| {
            let script = format!("jq -r '{BAD_JQ_PRECEDENCE_FILTER}' ledger.json");
            let output = commands::shell_cmd::run(&strings(&["-lc", &script])).unwrap();

            assert_eq!(output.exit_code, 5);
            assert!(output.stdout.contains("[full output:"));
            assert!(output.stderr.contains("jq comma branches"));
            assert!(output.stderr.contains(REPAIRED_JQ_PRECEDENCE_FILTER));
        },
    );
}

#[test]
fn explicit_passthrough_rewrites_remote_python_heredoc_to_ssh_stdin() {
    with_fake_tools(
        &[(
            "ssh",
            r#"#!/bin/sh
if [ "$1" != "build-host-a" ] || [ "$2" != "python3" ] || [ "$3" != "-" ]; then
  printf 'unexpected ssh args: %s\n' "$*" >&2
  exit 9
fi
printf 'host=%s\n' "$1"
printf 'remote=%s %s\n' "$2" "$3"
index=1
while IFS= read -r line || [ -n "$line" ]; do
  printf 'stdin[%s]=%s\n' "$index" "$line"
  index=$((index + 1))
done
"#,
        )],
        |temp| {
            let db_path = temp.join("insights.sqlite");
            let remote = "python3 - <<'PY'\nprint('alpha')\np = '/Users/example/project'\nPY";
            let output = cx_test_command()
                .arg("--")
                .arg("ssh")
                .arg("build-host-a")
                .arg(remote)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env("CX_INSIGHTS_DB_PATH", &db_path)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();

            assert!(
                output.status.success(),
                "stderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("host=build-host-a"));
            assert!(stdout.contains("remote=python3 -"));
            assert!(stdout.contains("stdin[1]=print('alpha')"));
            assert!(stdout.contains("stdin[2]=p = '/Users/example/project'"));
            assert!(output.stderr.is_empty());
        },
    );
}

#[test]
fn explicit_passthrough_rejects_ambiguous_remote_python_heredoc_before_ssh() {
    with_fake_tools(
        &[(
            "ssh",
            "#!/bin/sh\nprintf 'ssh should not run\\n' >&2\nexit 77\n",
        )],
        |temp| {
            let db_path = temp.join("insights.sqlite");
            let output = cx_test_command()
                .arg("--")
                .arg("ssh")
                .arg("build-host-a")
                .arg("python3 - <<'PY' print('alpha') PY")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
                .env("CX_INSIGHTS_DB_PATH", &db_path)
                .env_remove("CX_DISABLE_INSIGHTS")
                .output()
                .unwrap();

            assert_eq!(output.status.code(), Some(2));
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("remote SSH heredoc"));
            assert!(stderr.contains("stdin forwarding"));
            assert!(stderr.contains("cx -- ssh <host> \"bash -s\""));
            assert!(!stderr.contains("ssh should not run"));
        },
    );
}

#[test]
fn recent_read_shapes_use_fixture_files() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("recent_fixture.rs");
    let content = (1..=230)
        .map(|line| {
            if line == 42 {
                "pub const FLAG_042: &str = \"--generate\";".to_string()
            } else if line == 200 {
                format!("pub const LONG_200: &str = \"{}\";", "A".repeat(420))
            } else {
                format!("pub fn line_{line:03}() {{}}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&file, content).unwrap();

    let normal = commands::read::run(&file, &commands::read::ReadOptions::default()).unwrap();
    assert_eq!(normal.exit_code, 0);
    assert!(normal.stdout.contains("line_001"));

    let range = commands::read::run(
        &file,
        &commands::read::ReadOptions {
            line_range: Some(commands::read::ReadRange {
                start: Some(1),
                end: Some(220),
            }),
            ..commands::read::ReadOptions::default()
        },
    )
    .unwrap();
    assert_eq!(range.exit_code, 0);
    assert!(range.stdout.contains("line_001"));
    assert!(range.stdout.contains("line_220"));

    let head = commands::read::run(
        &file,
        &commands::read::ReadOptions {
            head_lines: Some(70),
            ..commands::read::ReadOptions::default()
        },
    )
    .unwrap();
    assert!(head.stdout.contains("FLAG_042"));
    assert!(!head.stdout.contains("line_090"));

    let raw_head = commands::read::run(
        &file,
        &commands::read::ReadOptions {
            raw: true,
            head_lines: Some(40),
            auto_aggressive: false,
            ..commands::read::ReadOptions::default()
        },
    )
    .unwrap();
    assert!(raw_head.stdout.contains("line_001"));
    assert!(raw_head.stdout.contains("line_040"));
    assert!(!raw_head.stdout.contains("FLAG_042"));

    let tail = commands::read::run(
        &file,
        &commands::read::ReadOptions {
            tail_lines: Some(4),
            ..commands::read::ReadOptions::default()
        },
    )
    .unwrap();
    assert!(tail.stdout.contains("line_230"));
    assert!(!tail.stdout.contains("line_001"));

    let raw_full = commands::read::run(
        &file,
        &commands::read::ReadOptions {
            raw: true,
            auto_aggressive: false,
            ..commands::read::ReadOptions::default()
        },
    )
    .unwrap();
    assert!(raw_full.stdout.contains("--generate"));
    assert!(raw_full.stdout.contains(&"A".repeat(420)));
}

#[test]
fn recent_grep_and_rg_shapes_preserve_match_evidence() {
    with_fake_tools(
        &[(
            "rg",
            &format!(
                "#!/bin/sh\n\
case \" $* \" in\n\
  *\" --files \"*) printf 'src/app.jsx\\nsrc/view.mjs\\nsrc/skip.generated.js\\n'; exit 0 ;;\n\
  *\" -l \"*) printf 'src/app.jsx\\nsrc/view.mjs\\n'; exit 0 ;;\n\
  *\" nomatch \"*) exit 1 ;;\n\
  *\" huge \"*) printf 'src/blob.mjs:8:{}\\n'; exit 0 ;;\n\
  *) printf 'src/app.jsx:3:const needle = true;\\nsrc/view.mjs:9:export const routeNeedle = needle;\\nsrc/menu.jsx:12:data-menu-open onClick\\n'; exit 0 ;;\n\
esac\n",
                "A".repeat(360)
            ),
        )],
        |_| {
            let fixed = commands::grep::run_many(
                &strings(&["data-menu-open", "onClick"]),
                &strings(&["src"]),
                &commands::grep::GrepOptions {
                    fixed_strings: true,
                    glob_patterns: strings(&["*.jsx"]),
                    max_results: Some(2),
                    ..commands::grep::GrepOptions::default()
                },
            )
            .unwrap();
            assert_eq!(fixed.exit_code, 0);
            assert!(fixed.stdout.contains("2 shown of 3 matches"));
            assert!(fixed.stdout.contains("[file] src/app.jsx"));
            assert!(fixed.stdout.contains("+1 more matches hidden by --max-results"));

            let extended = commands::grep::run_many(
                &strings(&["foo|bar|target_sources\\("]),
                &strings(&["src"]),
                &commands::grep::GrepOptions {
                    extended_regexp: true,
                    ..commands::grep::GrepOptions::default()
                },
            )
            .unwrap();
            assert_eq!(
                extended.stdout,
                "src/app.jsx:3:const needle = true;\nsrc/view.mjs:9:export const routeNeedle = needle;\nsrc/menu.jsx:12:data-menu-open onClick"
            );

            let files_with_matches = commands::grep::run_many(
                &strings(&["needle"]),
                &strings(&["src"]),
                &commands::grep::GrepOptions {
                    files_with_matches: true,
                    ..commands::grep::GrepOptions::default()
                },
            )
            .unwrap();
            assert_eq!(
                files_with_matches.stdout,
                "src/app.jsx\nsrc/view.mjs\n"
            );

            let listed = commands::grep::list_files(
                &strings(&["src"]),
                &commands::grep::GrepOptions {
                    max_results: Some(2),
                    ..commands::grep::GrepOptions::default()
                },
            )
            .unwrap();
            assert!(listed.stdout.contains("2 shown of 3 files"));
            assert!(listed.stdout.contains("+1 more files hidden by --max-results"));

            let huge = commands::grep::run_many(
                &strings(&["huge"]),
                &strings(&["src"]),
                &commands::grep::GrepOptions::default(),
            )
            .unwrap();
            assert!(huge.stdout.starts_with("src/blob.mjs:8:"));
            assert!(huge.stdout.contains("[truncated]"));
            assert!(!huge.stdout.contains(&"A".repeat(300)));

            let no_match = commands::grep::run_many(
                &strings(&["nomatch"]),
                &strings(&["src"]),
                &commands::grep::GrepOptions::default(),
            )
            .unwrap();
            assert_eq!(no_match.exit_code, 1);
            assert!(no_match.stdout.contains("0 matches"));
        },
    );
}

#[test]
fn recent_find_shapes_use_safe_fixture_tree() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join("src/nested")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("build/debug")).unwrap();
    fs::create_dir_all(root.join("source")).unwrap();
    fs::write(root.join("src/app.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/route.MJS"), "export default 1;\n").unwrap();
    fs::write(
        root.join("src/nested/native_core.mjs"),
        "export const ok = true;\n",
    )
    .unwrap();
    fs::write(root.join("build/debug/sample-service"), "binary\n").unwrap();
    fs::write(root.join("build/debug/other-service"), "binary\n").unwrap();
    fs::write(root.join("source/sample-service"), "source\n").unwrap();
    let executable = root.join("bin/tool");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();

    let by_name = commands::find::run(&strings(&[
        root.to_str().unwrap(),
        "-maxdepth",
        "1",
        "-type",
        "f",
        "-name",
        "*.rs",
        "-o",
        "-iname",
        "*.mjs",
    ]))
    .unwrap();
    assert_eq!(by_name.exit_code, 0);
    assert!(by_name.stdout.contains("app.rs"));
    assert!(by_name.stdout.contains("route.MJS"));
    assert!(!by_name.stdout.contains("native_core.mjs"));

    let by_path = commands::find::run(&strings(&[
        root.to_str().unwrap(),
        "-type",
        "f",
        "-path",
        "*/nested/*",
    ]))
    .unwrap();
    assert!(by_path.stdout.contains("native_core.mjs"));

    let path_first = commands::find::run(&strings(&[
        root.to_str().unwrap(),
        "-path",
        "*/nested/*",
        "-maxdepth",
        "3",
    ]))
    .unwrap();
    assert!(path_first.stdout.contains("native_core.mjs"));

    let path_and_name = commands::find::run(&strings(&[
        root.to_str().unwrap(),
        "-path",
        "*/build/*",
        "-type",
        "f",
        "-name",
        "sample-service",
    ]))
    .unwrap();
    assert!(path_and_name.stdout.contains("build/debug/sample-service"));
    assert!(!path_and_name.stdout.contains("other-service"));
    assert!(!path_and_name.stdout.contains("source/sample-service"));

    let executable_match = commands::find::run(&strings(&[
        root.to_str().unwrap(),
        "-type",
        "f",
        "-perm",
        "-111",
    ]))
    .unwrap();
    assert!(executable_match.stdout.contains("bin/tool"));
}

#[test]
fn recent_node_check_shapes_keep_node_truth_for_mjs_and_jsx() {
    with_fake_tools(
        &[(
            "node",
            r#"#!/bin/sh
if [ "$1" != "--check" ]; then
  printf 'unexpected node args: %s\n' "$*" >&2
  exit 7
fi
case "$2" in
  *native_core.mjs)
    printf '%s:1 import manifest from "../packages/manifest.json" assert { type: "json" };\n^^^^^^ SyntaxError: Unexpected identifier '\''assert'\''\nNode.js v24.14.1\n' "$2" >&2
    exit 1
    ;;
  *good.js|*good.mjs)
    exit 0
    ;;
  *)
    printf 'unexpected file: %s\n' "$2" >&2
    exit 8
    ;;
esac
"#,
        )],
        |root| {
            let fixture = root.join("node");
            fs::create_dir_all(&fixture).unwrap();
            let js = fixture.join("good.js");
            let jsx = fixture.join("widget.jsx");
            let mjs = fixture.join("native_core.mjs");
            fs::write(&js, "const ok = true;\n").unwrap();
            fs::write(
                &jsx,
                "export default function Widget() { return <div data-menu-open />; }\n",
            )
            .unwrap();
            fs::write(
                &mjs,
                "import manifest from \"../packages/manifest.json\" assert { type: \"json\" };\n",
            )
            .unwrap();

            let output = commands::node_cmd::run(&strings(&[
                "--check",
                js.to_str().unwrap(),
                jsx.to_str().unwrap(),
                mjs.to_str().unwrap(),
            ]))
            .unwrap();
            assert_eq!(output.exit_code, 1);
            assert!(output.stdout.contains("good.js"));
            assert!(output.stdout.contains("[jsx parser]"));
            assert!(output.stderr.contains("SyntaxError: Unexpected identifier"));
            assert!(output.stderr.contains("Node.js v24.14.1"));

            let unsupported =
                commands::node_cmd::run(&strings(&["--check", "--input-type=module"])).unwrap_err();
            assert!(unsupported.to_string().contains("unsupported argument"));
        },
    );
}

#[test]
fn recent_git_shapes_forward_expected_arguments() {
    with_fake_tools(
        &[(
            "git",
            r#"#!/bin/sh
case "$1" in
  status)
    if [ "$2" = "--porcelain" ]; then
      printf '## main\nM  src/lib.rs\n?? tests/recent_calls.rs\n'
    else
      printf ' M src/lib.rs\n'
    fi
    ;;
  diff)
    if [ "$2" = "--stat" ]; then
      printf ' src/lib.rs | 2 +-\n 1 file changed, 1 insertion(+), 1 deletion(-)\n'
    elif [ "$2" = "--name-only" ]; then
      printf 'conflicted.js\n'
    else
      printf 'diff --git a/src/lib.rs b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n'
    fi
    ;;
  log)
    case " $* " in
      *" --oneline "*) printf 'abc123 r10 Improve grep handling\ndef456 r9 Support find path predicates\n' ;;
      *) printf 'abc123 r10 Improve grep handling (2 hours ago) <dev>\nbody line\n---END---\n' ;;
    esac
    ;;
  show)
    for arg in "$@"; do
      if [ "$arg" = "--no-compact" ]; then
        printf 'git show received cx-only flag\n' >&2
        exit 17
      fi
    done
    printf 'commit abc123\n\n    r10 Improve grep handling\n---\n src/lib.rs | 2 +-\n@@ -1 +1 @@\n-old\n+new\n'
    ;;
  *)
    printf 'unexpected git args: %s\n' "$*" >&2
    exit 9
    ;;
esac
"#,
        )],
        |_| {
            let status = commands::git::run_status(&[]).unwrap();
            assert!(status.stdout.contains("* main"));
            assert!(status.stdout.contains("src/lib.rs"));

            let short = commands::git::run_status(&strings(&["--short"])).unwrap();
            assert!(short.stdout.contains("M src/lib.rs"));

            let diff = commands::git::run_diff(&[]).unwrap();
            assert!(diff.stdout.contains("src/lib.rs"));
            assert!(diff.stdout.contains("+1 -1"));

            let stat = commands::git::run_diff(&strings(&["--stat"])).unwrap();
            assert!(stat.stdout.contains("1 file changed"));

            let unmerged =
                commands::git::run_diff(&strings(&["--name-only", "--diff-filter=U"])).unwrap();
            assert_eq!(unmerged.stdout, "conflicted.js");
            assert!(!unmerged.stdout.contains("--- Changes ---"));

            let path_diff = commands::git::run_diff(&strings(&["--", "src/lib.rs"])).unwrap();
            assert!(path_diff.stdout.contains("src/lib.rs"));

            let log = commands::git::run_log(&strings(&["-n", "3"])).unwrap();
            assert!(log.stdout.contains("r10 Improve grep handling"));
            assert!(log.stdout.contains("body line"));

            let oneline = commands::git::run_log(&strings(&["-n", "8", "--oneline"])).unwrap();
            assert!(oneline.stdout.contains("abc123 r10"));

            let compact_limit = commands::git::run_log(&strings(&["-5", "--oneline"])).unwrap();
            assert!(compact_limit.stdout.contains("def456 r9"));

            let graph_all = commands::git::run_log(&strings(&[
                "--oneline",
                "--decorate",
                "--graph",
                "--max-count=20",
                "--all",
            ]))
            .unwrap();
            assert!(graph_all.stdout.contains("abc123 r10"));

            let show = commands::git::run_show(&strings(&[
                "--no-compact",
                "--no-ext-diff",
                "--no-color",
                "--format=medium",
                "--stat",
                "--patch",
                "HEAD",
            ]))
            .unwrap();
            assert_eq!(show.exit_code, 0);
            assert!(show.stdout.contains("r10 Improve grep handling"));
            assert!(!show.stderr.contains("cx-only flag"));

            let top_level_diff = Cli::try_parse_from(["cx", "diff", "--", "src/lib.rs"]).unwrap();
            assert_eq!(dispatch::execute(&top_level_diff).unwrap(), 0);
        },
    );
}

#[test]
fn recent_git_evidence_diff_shape_preserves_evidence_args() {
    with_fake_tools(
        &[(
            "git",
            r#"#!/bin/sh
case "$1:$2" in
  rev-parse:--verify)
    exit 0
    ;;
  show:--format=)
    printf 'evidence-diff args:%s\n+head-evidence\n' "$*"
    ;;
  diff:--no-ext-diff)
    printf 'evidence-diff args:%s\n+range-evidence\n' "$*"
    ;;
  *)
    printf 'unexpected git args: %s\n' "$*" >&2
    exit 9
    ;;
esac
"#,
        )],
        |_| {
            let evidence_diff = commands::git::run_evidence_diff(&[]).unwrap();
            assert_eq!(evidence_diff.exit_code, 0);
            assert!(evidence_diff
                .stdout
                .contains("evidence-diff args:diff --no-ext-diff --no-color HEAD^..HEAD"));
            assert!(evidence_diff.stdout.contains("+range-evidence"));

            let evidence_range =
                commands::git::run_evidence_diff(&strings(&["HEAD~1..HEAD", "--", "src/lib.rs"]))
                    .unwrap();
            assert_eq!(evidence_range.exit_code, 0);
            assert!(evidence_range.stdout.contains(
                "evidence-diff args:diff --no-ext-diff --no-color HEAD~1..HEAD -- src/lib.rs"
            ));
            assert!(evidence_range.stdout.contains("+range-evidence"));

            let evidence_range_without_paths =
                commands::git::run_evidence_diff(&strings(&["HEAD^..HEAD"])).unwrap();
            assert_eq!(evidence_range_without_paths.exit_code, 0);
            assert!(evidence_range_without_paths
                .stdout
                .contains("evidence-diff args:diff --no-ext-diff --no-color HEAD^..HEAD"));
            assert!(evidence_range_without_paths
                .stdout
                .contains("+range-evidence"));
        },
    );
}

#[test]
fn recent_git_conflict_diff_shape_uses_stage_blob_specs() {
    with_fake_tools(
        &[(
            "git",
            r#"#!/bin/sh
if [ "$1" = "diff" ] && [ "$2" = "--stat" ]; then
  printf 'stat:%s:%s\n' "$3" "$4"
  exit 0
fi
printf 'unexpected git args: %s\n' "$*" >&2
exit 9
"#,
        )],
        |_| {
            let output = commands::git::run_conflict_diff(&strings(&[
                "--stat",
                "packages/chat-ui/src/chat/ChatState.js",
            ]))
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains(
            "stat::2:packages/chat-ui/src/chat/ChatState.js::3:packages/chat-ui/src/chat/ChatState.js"
            ));
        },
    );
}

#[test]
fn recent_shell_wrapper_runs_bash_command() {
    with_fake_tools(
        &[(
            "bash",
            "#!/bin/sh\nif [ \"$1\" = \"-lc\" ]; then shift; /bin/sh -c \"$1\"; else /bin/cat; fi\n",
        )],
        |_| {
            let output = commands::shell_cmd::run(&strings(&["-lc", "printf shell-ok"])).unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(output.stdout.trim(), "shell-ok");
        },
    );
}

#[test]
fn recent_cmake_and_cargo_shapes_use_fake_binaries() {
    with_fake_tools(
        &[
            (
                "cmake",
                r#"#!/bin/sh
case " $* " in
  *" sample-ui-web-service "*) printf 'error: no such file or directory: generated_asset.bin\n' >&2; exit 2 ;;
  *) printf '[100%%] Built target sample-ui\n'; exit 0 ;;
esac
"#,
            ),
            (
                "cargo",
                r#"#!/bin/sh
if [ "$1" != "test" ]; then
  printf 'unexpected cargo args: %s\n' "$*" >&2
  exit 9
fi
printf '%s\n' \
  'running 1 test' \
  'failures:' \
  '---- recent_call_regression stdout ----' \
  "thread 'recent_call_regression' panicked at src/lib.rs:10:5:" \
  'assertion failed: output.contains("evidence")' \
  '' \
  'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out'
exit 101
"#,
            ),
        ],
        |_| {
            let build_ok = commands::cmake_cmd::run_build(&strings(&[
                "app/sample-ui/build-web",
                "--target",
                "sample-ui",
                "-j",
                "8",
            ]))
            .unwrap();
            assert_eq!(build_ok.exit_code, 0);
            assert!(build_ok.stdout.contains("cmake build: ok"));
            assert!(build_ok.stdout.contains("Built target sample-ui"));

            let build_failed = commands::cmake_cmd::run_build(&strings(&[
                "app/sample-ui/build-web",
                "--target",
                "sample-ui-web-service",
            ]))
            .unwrap();
            assert_eq!(build_failed.exit_code, 2);
            assert!(build_failed.stdout.contains("cmake build: failed"));
            assert!(build_failed.stdout.contains("no such file"));

            let cargo = commands::cargo_cmd::run_test(&[]).unwrap();
            assert_eq!(cargo.exit_code, 101);
            assert!(cargo.stdout.contains("recent_call_regression"));
            assert!(cargo.stdout.contains("test result: FAILED"));
        },
    );
}

#[test]
fn native_cmake_build_auto_shape_uses_the_compact_wrapper() {
    with_fake_tools(
        &[(
            "cmake",
            r#"#!/bin/sh
if [ "$1" != "--build" ] || [ "$2" != "build-web" ]; then
  printf 'unexpected cmake args: %s\n' "$*" >&2
  exit 9
fi
i=0
while [ "$i" -lt 90 ]; do
  printf '[%3d%%] Building CXX object src/CMakeFiles/sample-ui.dir/noise_%03d.cpp.o\n' "$i" "$i"
  i=$((i + 1))
done
printf '[100%%] Built target sample-ui\n'
"#,
        )],
        |temp| {
            let db_path = temp.join("db.sqlite");
            let output = cx_test_command()
                .args([
                    "--",
                    "cmake",
                    "--build",
                    "build-web",
                    "--target",
                    "sample-ui",
                    "-j8",
                ])
                .env("CX_INSIGHTS_DB_PATH", &db_path)
                .env_remove("CX_DISABLE_INSIGHTS")
                .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains("cmake build: ok"));
            assert!(stdout.contains("Built target sample-ui"));
            assert!(stdout.contains("lines omitted"));

            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let identity = connection
                .query_row(
                    "SELECT process, command_family FROM command_invocations ORDER BY id DESC LIMIT 1",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .unwrap();
            assert_eq!(identity, ("cmake".to_string(), "cmake build".to_string()));
        },
    );
}

#[test]
fn native_ps_auto_shape_compacts_inventory_and_records_official_identity() {
    with_fake_tools(
        &[(
            "ps",
            r#"#!/bin/sh
if [ "$1" != "-axo" ] || [ "$2" != "pid,ppid,etime,command" ]; then
  printf 'unexpected ps args: %s\n' "$*" >&2
  exit 9
fi
printf 'PID PPID ELAPSED COMMAND\n'
i=1
while [ "$i" -le 120 ]; do
  printf '%s 1 00:01 /usr/bin/node app-%s.mjs\n' "$i" "$i"
  i=$((i + 1))
done
printf '501 1 00:03 /opt/tools/rare-service --token=sk-secretsecret\n'
"#,
        )],
        |temp| {
            let db_path = temp.join("db.sqlite");
            let output = cx_test_command()
                .args(["--", "ps", "-axo", "pid,ppid,etime,command"])
                .env("CX_INSIGHTS_DB_PATH", &db_path)
                .env_remove("CX_DISABLE_INSIGHTS")
                .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains("ps: 121 processes across 2 executables"));
            assert!(stdout.contains("node | 120"));
            assert!(stdout.contains("rare-service"));
            assert!(stdout.contains("[REDACTED]"));
            assert!(stdout.contains("[full process table: cx -- ps --no-compact"));

            let connection = rusqlite::Connection::open(&db_path).unwrap();
            let identity = connection
                .query_row(
                    "SELECT process, command_family, raw_tokens, emitted_tokens FROM command_invocations ORDER BY id DESC LIMIT 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(identity.0, "ps");
            assert_eq!(identity.1, "ps");
            assert!(identity.2 > identity.3);
        },
    );
}

#[test]
fn recent_ctest_shapes_use_fake_binaries() {
    with_fake_tools(
        &[(
            "ctest",
            r#"#!/bin/sh
case " $* " in
  *" -N "*)
    printf '%s\n' \
      'Test project build-web' \
      '  Test #1: sample-ui-catalog-model-test' \
      '  Test #2: sample-ui-chat-markdown-model-tests' \
      'Total Tests: 2'
    exit 0
    ;;
esac
printf '%s\n' \
  'Test project build-web' \
  '1/2 Test #1: sample-ui-dockview-perf-mouse-e2e ....***Failed 0.42 sec' \
  '50% tests passed, 1 tests failed out of 2' \
  'The following tests FAILED:' \
  '        1 - sample-ui-dockview-perf-mouse-e2e (Failed)' \
  'Errors while running CTest'
exit 8
"#,
        )],
        |_| {
            let ctest = commands::ctest_cmd::run(&strings(&[
                "--test-dir",
                "app/sample-ui/build-web",
                "-R",
                "sample-ui-dockview-perf-mouse-e2e",
                "--output-on-failure",
            ]))
            .unwrap();
            assert_eq!(ctest.exit_code, 8);
            assert!(ctest.stdout.contains("ctest: failed"));
            assert!(ctest.stdout.contains("sample-ui-dockview-perf-mouse-e2e"));

            let ctest_list = commands::ctest_cmd::run(&strings(&[
                "--test-dir",
                "app/sample-ui/build-web",
                "-N",
                "-R",
                "sample-ui-catalog-model-test",
            ]))
            .unwrap();
            assert_eq!(ctest_list.exit_code, 0);
            assert_eq!(
                ctest_list.stdout,
                "Test project build-web\n  Test #1: sample-ui-catalog-model-test\n  Test #2: sample-ui-chat-markdown-model-tests\nTotal Tests: 2\n"
            );
        },
    );
}

#[test]
fn recent_tsc_and_ls_shapes_use_fake_binaries() {
    with_fake_tools(
        &[
            (
                "tsc",
                r#"#!/bin/sh
printf 'shared/webgpu/src/runtime_core/native_core.mjs(4,10): error TS2322: Type '\''number'\'' is not assignable to type '\''string'\''.\n'
exit 2
"#,
            ),
            (
                "ls",
                r#"#!/bin/sh
printf '%s\n' \
  'total 8' \
  'drwxr-xr-x  2 user  staff   64 Jan 1 12:00 src' \
  'drwxr-xr-x  2 user  staff   64 Jan 1 12:00 node_modules' \
  '-rw-r--r--  1 user  staff 1234 Jan 1 12:00 Cargo.toml' \
  '-rw-r--r--  1 user  staff  120 Jan 1 12:00 README.md'
"#,
            ),
        ],
        |_| {
            let tsc = commands::tsc_cmd::run(&strings(&[
                "--noEmit",
                "--project",
                "targets/web_browser/tsconfig.json",
            ]))
            .unwrap();
            assert_eq!(tsc.exit_code, 2);
            assert!(tsc.stdout.contains("TypeScript: 1 errors"));
            assert!(tsc.stdout.contains("TS2322"));
            assert!(tsc.stdout.contains("native_core.mjs"));

            let ls = commands::ls::run(&strings(&["/Users/example/repos/project"])).unwrap();
            assert_eq!(ls.exit_code, 0);
            assert!(ls.stdout.contains("src/"));
            assert!(ls.stdout.contains("Cargo.toml"));
            assert!(!ls.stdout.contains("node_modules"));
            assert!(ls.stdout.contains("Summary: 2 files, 1 dirs"));
        },
    );
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_fake_tools<T>(entries: &[(&str, &str)], action: impl FnOnce(&Path) -> T) -> T {
    let _lock = env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    for (name, body) in entries {
        write_executable(&bin, name, body);
    }

    let _env = EnvGuard::set(&bin, &home);
    action(temp.path())
}

fn cx_test_command() -> Command {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    let home = HOME.get_or_init(|| tempdir().expect("cx test home"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
    command.env("HOME", home.path());
    command
}

fn write_executable(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
}

struct EnvGuard {
    path: Option<OsString>,
    disable_fallback: Option<OsString>,
    extra_fallback: Option<OsString>,
    home: Option<OsString>,
}

impl EnvGuard {
    fn set(bin: &Path, home: &Path) -> Self {
        let guard = Self {
            path: env::var_os("PATH"),
            disable_fallback: env::var_os("CX_DISABLE_TOOL_FALLBACK_PATHS"),
            extra_fallback: env::var_os("CX_TOOL_FALLBACK_PATHS"),
            home: env::var_os("HOME"),
        };
        env::set_var("PATH", bin);
        env::set_var("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
        env::remove_var("CX_TOOL_FALLBACK_PATHS");
        env::set_var("HOME", home);
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        restore_env("PATH", self.path.take());
        restore_env(
            "CX_DISABLE_TOOL_FALLBACK_PATHS",
            self.disable_fallback.take(),
        );
        restore_env("CX_TOOL_FALLBACK_PATHS", self.extra_fallback.take());
        restore_env("HOME", self.home.take());
    }
}

fn restore_env(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => env::set_var(name, value),
        None => env::remove_var(name),
    }
}
