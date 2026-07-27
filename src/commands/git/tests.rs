use super::diff::{compact_diff, materialize_no_index_fd_args, no_index_fd_positions};
use super::evidence::{parse_conflict_diff_args, parse_evidence_diff_args, EvidenceDiffRevision};
use super::history::filter_log_output;
use super::status::format_status_output;
use super::*;
use std::fs;
use std::time::{Duration, Instant};

use tempfile::tempdir;

#[test]
fn format_status_output_compacts_porcelain() {
    let porcelain = "## main\nM  src/lib.rs\n?? Cargo.lock\n";
    let formatted = format_status_output(porcelain);
    assert!(formatted.contains("* main"));
    assert!(formatted.contains("+ Staged: 1 files"));
    assert!(formatted.contains("? Untracked: 1 files"));
}

#[test]
fn compact_diff_truncates_long_hunks() {
    let diff = "diff --git a/src/lib.rs b/src/lib.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
    let compacted = compact_diff(diff, 20);
    assert!(compacted.contains("src/lib.rs"));
    assert!(compacted.contains("+1 -1"));
}

#[test]
fn parse_conflict_diff_defaults_to_stage_two_and_three() {
    let options =
        parse_conflict_diff_args(&["--stat".to_string(), "packages/chat.js".to_string()]).unwrap();
    assert_eq!(options.left_stage, "2");
    assert_eq!(options.right_stage, "3");
    assert_eq!(options.diff_args, ["--stat"]);
    assert_eq!(options.paths, ["packages/chat.js"]);
}

#[test]
fn parse_conflict_diff_accepts_explicit_stage_pair() {
    let options = parse_conflict_diff_args(&[
        "--stage=1:3".to_string(),
        "--no-compact".to_string(),
        "--".to_string(),
        "packages/chat.js".to_string(),
    ])
    .unwrap();
    assert_eq!(options.left_stage, "1");
    assert_eq!(options.right_stage, "3");
    assert_eq!(options.diff_args, ["--no-compact"]);
    assert_eq!(options.paths, ["packages/chat.js"]);
}

#[test]
fn parse_evidence_diff_defaults_to_head_commit() {
    let options = parse_evidence_diff_args(&[]).unwrap();
    assert_eq!(
        options.revision,
        EvidenceDiffRevision::Commit("HEAD".to_string())
    );
    assert!(options.paths.is_empty());
}

#[test]
fn parse_evidence_diff_accepts_range_and_pathspec_separator() {
    let options = parse_evidence_diff_args(&[
        "HEAD~2..HEAD".to_string(),
        "--".to_string(),
        "src/lib.rs".to_string(),
        "tests/recent_calls.rs".to_string(),
    ])
    .unwrap();
    assert_eq!(
        options.revision,
        EvidenceDiffRevision::Range("HEAD~2..HEAD".to_string())
    );
    assert_eq!(options.paths, ["src/lib.rs", "tests/recent_calls.rs"]);
}

#[test]
fn parse_evidence_diff_accepts_default_revision_with_paths() {
    let options = parse_evidence_diff_args(&["--".to_string(), "src/lib.rs".to_string()]).unwrap();
    assert_eq!(
        options.revision,
        EvidenceDiffRevision::Commit("HEAD".to_string())
    );
    assert_eq!(options.paths, ["src/lib.rs"]);
}

#[test]
fn parse_evidence_diff_rejects_options_and_multiple_revisions() {
    let option_error = parse_evidence_diff_args(&["--stat".to_string()]).unwrap_err();
    assert!(option_error
        .to_string()
        .contains("optional commit or range"));

    let multi_error =
        parse_evidence_diff_args(&["HEAD".to_string(), "main".to_string()]).unwrap_err();
    assert!(multi_error.to_string().contains("only one commit/range"));
}

#[test]
fn no_index_fd_positions_recognizes_two_fd_inputs_only() {
    let args = vec![
        "--no-index".to_string(),
        "--stat".to_string(),
        "/dev/fd/11".to_string(),
        "/dev/fd/12".to_string(),
    ];
    assert_eq!(no_index_fd_positions(&args), Some([2, 3]));

    let mixed = vec![
        "--no-index".to_string(),
        "/dev/fd/11".to_string(),
        "normal-file".to_string(),
    ];
    assert_eq!(no_index_fd_positions(&mixed), None);
}

#[cfg(unix)]
#[test]
fn run_diff_skips_optional_optimizations_when_setting_is_disabled() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "git",
            "#!/bin/sh\nif [ \"$1\" = \"diff\" ] && [ \"$2\" = \"--stat\" ]; then\nprintf 'unexpected compact stat path\\n' >&2\nexit 17\nfi\nif [ \"$1\" = \"diff\" ]; then\nprintf 'direct:%s\\n' \"$*\"\nexit 0\nfi\nexit 9\n",
        );

    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(bin.to_string_lossy().as_ref())),
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_INSIGHTS_DB_PATH", None),
            ("CX_ENABLE_INSIGHTS", None),
            ("CX_DISABLE_INSIGHTS", None),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            crate::support::insights::set_insight_setting("command_optimizations", "false")
                .unwrap();

            let output = run_diff(&[]).unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(output.stdout, "direct:diff");
            assert!(!output.stdout.contains("--- Changes ---"));

            let fd_output = run_diff(&[
                "--no-index".to_string(),
                "/dev/fd/11".to_string(),
                "/dev/fd/12".to_string(),
            ])
            .unwrap();
            assert_eq!(fd_output.exit_code, 0);
            assert!(fd_output.stdout.contains("/dev/fd/11"));
            assert!(fd_output.stdout.contains("/dev/fd/12"));
        },
    );
}

#[test]
fn filter_log_output_compacts_commit_blocks() {
    let output = "abc123 message (1 day ago) <dev>\nbody\n---END---";
    let filtered = filter_log_output(output, 10, false);
    assert!(filtered.contains("abc123 message"));
    assert!(filtered.contains("body"));
}

#[cfg(unix)]
#[test]
fn run_log_applies_default_no_merge_policy_only_to_cx_formatted_history() {
    assert_run_log_argv(
        &[],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --no-merges",
    );
    assert_run_log_argv(
        &["-n", "3"],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- --no-merges -n 3",
    );
}

#[cfg(unix)]
#[test]
fn run_log_preserves_user_format_and_parent_policy_argv() {
    assert_run_log_argv(&["-n", "1", "--oneline"], "log -n 1 --oneline");
    assert_run_log_argv(
        &["--first-parent", "--oneline", "--decorate", "-n", "3"],
        "log --first-parent --oneline --decorate -n 3",
    );
    assert_run_log_argv(
        &["--first-parent"],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --first-parent",
    );
    assert_run_log_argv(
        &["--merges"],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --merges",
    );
    assert_run_log_argv(
        &["--min-parents=2"],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --min-parents=2",
    );
    assert_run_log_argv(
        &["--max-parents", "1"],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --max-parents 1",
    );
    assert_run_log_argv(
        &["--no-merges"],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --no-merges",
    );
    assert_run_log_argv(
        &["--no-max-parents"],
        "log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --no-max-parents",
    );
}

#[cfg(unix)]
#[test]
fn run_log_preserves_user_formatted_output_exactly() {
    const EXPECTED: &str = "6c0cc68c46d723b18bb83533453232a8d6193d87\t2953dd73c5735fd1ce9e3c6182fa32f1dd7f4f0e\tcontextlimit\tcontextlimit@users.noreply.github.com\t1784264100\tHEAD -> main, tag: r119\tr119 Harden machine-readable output and insights concurrency with a deliberately long fixture suffix\n";
    let script = format!(
        "#!/bin/sh\nif [ \"$*\" != 'log -1 --format=%H%x09%P%x09%an%x09%ae%x09%at%x09%D%x09%s' ]; then\nprintf 'unexpected git args: %s\\n' \"$*\" >&2\nexit 97\nfi\nprintf 'fixture warning\\n' >&2\ncat <<'EOF'\n{EXPECTED}EOF\n"
    );
    crate::support::test_support::with_fake_path(&[("git", &script)], || {
        let output = run_log(&[
            "-1".to_string(),
            "--format=%H%x09%P%x09%an%x09%ae%x09%at%x09%D%x09%s".to_string(),
        ])
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, EXPECTED);
        assert_eq!(output.stderr, "fixture warning\n");
        assert!(output
            .observation
            .as_ref()
            .is_some_and(|observation| observation.uses_preserved_stream_termination()));
    });
}

#[cfg(unix)]
#[test]
fn run_log_strips_no_compact_and_preserves_native_output_exactly() {
    const EXPECTED: &str = "commit 37b762f509c\nAuthor: Context Limit\n\n    exact history\n";
    let script = format!(
        "#!/bin/sh\nif [ \"$*\" != 'log -1 --format=fuller 37b762f509c' ]; then\nprintf 'unexpected git args: %s\\n' \"$*\" >&2\nexit 97\nfi\ncat <<'EOF'\n{EXPECTED}EOF\n"
    );
    crate::support::test_support::with_fake_path(&[("git", &script)], || {
        let output = run_log(&[
            "--no-compact".to_string(),
            "-1".to_string(),
            "--format=fuller".to_string(),
            "37b762f509c".to_string(),
        ])
        .unwrap();
        assert_eq!(output.exit_code, 0, "{}", output.stderr);
        assert_eq!(output.stdout, EXPECTED);
        assert!(output.stderr.is_empty());
        assert!(output
            .observation
            .as_ref()
            .is_some_and(|observation| observation.uses_preserved_stream_termination()));
    });
}

#[cfg(unix)]
fn assert_run_log_argv(args: &[&str], expected: &str) {
    assert!(
        !expected.contains('\''),
        "fixture uses shell single-quote delimiters"
    );
    let script = format!(
        "#!/bin/sh\nif [ \"$*\" != '{expected}' ]; then\nprintf '%s\\n' 'expected: {expected}' >&2\nprintf 'actual:   %s\\n' \"$*\" >&2\nexit 97\nfi\nprintf 'abc123 message (1 day ago) <dev>\\n---END---\\n'\n"
    );
    crate::support::test_support::with_fake_path(&[("git", &script)], || {
        let args = args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>();
        let output = run_log(&args).unwrap();
        assert_eq!(output.exit_code, 0, "{}", output.stderr);
    });
}

#[cfg(unix)]
#[test]
fn materialize_no_index_fd_args_copies_open_fd_inputs() {
    use std::os::fd::AsRawFd;

    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let left_path = temp.path().join("left.txt");
    let right_path = temp.path().join("right.txt");
    fs::write(&left_path, "left\n").unwrap();
    fs::write(&right_path, "right\n").unwrap();
    let left = fs::File::open(&left_path).unwrap();
    let right = fs::File::open(&right_path).unwrap();
    let left_fd = format!("/dev/fd/{}", left.as_raw_fd());
    let right_fd = format!("/dev/fd/{}", right.as_raw_fd());

    crate::support::test_support::with_env_vars(
        &[("HOME", Some(home.to_string_lossy().as_ref()))],
        || {
            let materialized =
                materialize_no_index_fd_args(&["--no-index".into(), left_fd, right_fd])
                    .unwrap()
                    .unwrap();
            assert_eq!(fs::read_to_string(&materialized.args[1]).unwrap(), "left\n");
            assert_eq!(
                fs::read_to_string(&materialized.args[2]).unwrap(),
                "right\n"
            );
            assert!(!materialized.args[1].starts_with("/dev/fd/"));
            assert!(!materialized.args[2].starts_with("/dev/fd/"));
        },
    );
}

#[cfg(unix)]
#[test]
fn run_evidence_diff_defaults_to_first_parent_range_without_compaction() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nif [ \"$1\" = \"rev-parse\" ]; then\nexit 0\nfi\nprintf 'ARGS:%s\\n' \"$*\"\ni=0\nwhile [ \"$i\" -lt 140 ]; do\nprintf '+evidence_line_%03d\\n' \"$i\"\ni=$((i + 1))\ndone\n",
            )],
            || {
                let output = run_evidence_diff(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output
                    .stdout
                    .contains("ARGS:diff --no-ext-diff --no-color HEAD^..HEAD"));
                assert!(output.stdout.contains("+evidence_line_139"));
                assert!(!output.stdout.contains("--- Changes ---"));
                assert!(!output.stdout.contains("[full diff:"));
                let observation = output.observation.unwrap();
                assert_eq!(observation.source, "git evidence-diff");
                assert!(observation.response.unwrap().contains("+evidence_line_139"));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_evidence_diff_falls_back_to_show_for_root_commit() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nif [ \"$1\" = \"rev-parse\" ]; then\nexit 1\nfi\nprintf 'ARGS:%s\\n+root-evidence\\n' \"$*\"\n",
            )],
            || {
                let output = run_evidence_diff(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains(
                    "ARGS:show --format= --no-ext-diff --no-color --patch HEAD"
                ));
                assert!(output.stdout.contains("+root-evidence"));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_evidence_diff_uses_git_diff_for_ranges_and_preserves_paths() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nprintf 'ARGS:%s\\n' \"$*\"\nprintf 'diff --git a/src/lib.rs b/src/lib.rs\\n+range\\n'\n",
            )],
            || {
                let output = run_evidence_diff(&[
                    "HEAD~2..HEAD".to_string(),
                    "--".to_string(),
                    "src/lib.rs".to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains(
                    "ARGS:diff --no-ext-diff --no-color HEAD~2..HEAD -- src/lib.rs"
                ));
                assert!(output.stdout.contains("+range"));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_conflict_diff_uses_stage_blob_specs() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nif [ \"$1\" = \"diff\" ] && [ \"$2\" = \"--stat\" ]; then\nprintf 'stat:%s:%s\\n' \"$3\" \"$4\"\nexit 0\nfi\nprintf 'unexpected git args: %s\\n' \"$*\" >&2\nexit 9\n",
            )],
            || {
                let output = run_conflict_diff(&[
                    "--stat".to_string(),
                    "packages/chat_ui_v1/src/chat/ChatState.js".to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains(
                    "stat::2:packages/chat_ui_v1/src/chat/ChatState.js::3:packages/chat_ui_v1/src/chat/ChatState.js"
                ));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_status_uses_fake_git_binary() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nif [ \"$2\" = \"--porcelain\" ]; then\ncat <<'EOF'\n## main\nM  src/lib.rs\nEOF\nelse\ncat <<'EOF'\nOn branch main\nnothing to commit, working tree clean\nEOF\nfi\n",
            )],
            || {
                let output = run_status(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("* main"));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_status_preserves_clean_short_output_as_empty() {
    crate::support::test_support::with_fake_path(
        &[(
            "git",
            "#!/bin/sh\nif [ \"$1\" = status ] && [ \"$2\" = --short ]; then exit 0; fi\nprintf 'unexpected git args: %s\\n' \"$*\" >&2\nexit 9\n",
        )],
        || {
            let output = run_status(&["--short".to_string()]).unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        },
    );
}

#[cfg(unix)]
#[test]
fn run_status_stores_failure_artifact() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "git",
        "#!/bin/sh\nprintf 'fatal: not a git repository\\n' >&2\nexit 128\n",
    );

    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(bin.to_string_lossy().as_ref())),
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let output = run_status(&[]).unwrap();
            assert_eq!(output.exit_code, 128);
            assert!(output.stderr.contains("not a git repository"));
            assert!(output
                .stdout
                .contains("[full output: ~/.cx/cache/failures/git/"));
            let artifact_dir = home.join(".cx/cache/failures/git");
            assert_eq!(fs::read_dir(artifact_dir).unwrap().count(), 1);
        },
    );
}

#[cfg(unix)]
#[test]
fn run_status_returns_without_waiting_for_descendant_stdout_to_close() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nif [ \"$1\" = \"status\" ]; then\n(sleep 1) &\nprintf '## main\\nM  src/lib.rs\\n'\nelse\nexit 9\nfi\n",
            )],
            || {
                let start = Instant::now();
                let output = run_status(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("* main"));
                assert!(output.stdout.contains("src/lib.rs"));
                assert!(start.elapsed() < Duration::from_millis(700));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_diff_returns_without_waiting_for_descendant_stdout_to_close() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nif [ \"$1\" = \"diff\" ] && [ \"$2\" = \"--stat\" ]; then\n(sleep 1) &\nprintf ' src/lib.rs | 2 +-\\n 1 file changed, 1 insertion(+), 1 deletion(-)\\n'\nelif [ \"$1\" = \"diff\" ]; then\n(sleep 1) &\nprintf 'diff --git a/src/lib.rs b/src/lib.rs\\n@@ -1 +1 @@\\n-old\\n+new\\n'\nelse\nexit 9\nfi\n",
            )],
            || {
                let start = Instant::now();
                let output = run_diff(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("src/lib.rs"));
                assert!(output.stdout.contains("+1 -1"));
                assert!(start.elapsed() < Duration::from_millis(700));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_diff_stores_artifact_when_full_diff_fails_after_stat() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "git",
            "#!/bin/sh\nif [ \"$1\" = \"diff\" ] && [ \"$2\" = \"--stat\" ]; then\nprintf ' src/lib.rs | 2 +-\\n 1 file changed, 1 insertion(+), 1 deletion(-)\\n'\nexit 0\nfi\nif [ \"$1\" = \"diff\" ]; then\nprintf 'fatal: external diff died\\n' >&2\nexit 129\nfi\nexit 7\n",
        );

    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(bin.to_string_lossy().as_ref())),
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let output = run_diff(&[]).unwrap();
            assert_eq!(output.exit_code, 129);
            assert!(output.stdout.contains("src/lib.rs"));
            assert!(output
                .stdout
                .contains("[full output: ~/.cx/cache/failures/git/"));
            assert!(output.stderr.contains("external diff died"));
            let artifact_dir = home.join(".cx/cache/failures/git");
            let artifacts = fs::read_dir(artifact_dir)
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(artifacts.len(), 1);
            let artifact_body = fs::read_to_string(artifacts[0].path()).unwrap();
            assert!(artifact_body.contains("1 file changed"));
            assert!(artifact_body.contains("external diff died"));
        },
    );
}

#[cfg(unix)]
#[test]
fn run_diff_no_index_difference_keeps_exit_without_failure_artifact() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "git",
            "#!/bin/sh\nif [ \"$1\" = \"diff\" ] && [ \"$2\" = \"--stat\" ]; then\nprintf ' left => right | 2 +-\\n 1 file changed, 1 insertion(+), 1 deletion(-)\\n'\nexit 1\nfi\nif [ \"$1\" = \"diff\" ]; then\nprintf 'diff --git a/left b/right\\n@@ -1 +1 @@\\n-left\\n+right\\n'\nexit 1\nfi\nexit 7\n",
        );

    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(bin.to_string_lossy().as_ref())),
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let output = run_diff(&[
                "--no-index".to_string(),
                "left.txt".to_string(),
                "right.txt".to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 1);
            assert!(output.stdout.contains("left => right"));
            assert!(output.stdout.contains("+1 -1"));
            assert!(!output.stdout.contains("[full output:"));
            let artifact_dir = home.join(".cx/cache/failures/git");
            assert!(!artifact_dir.exists());
        },
    );
}

#[cfg(unix)]
#[test]
fn run_log_returns_without_waiting_for_descendant_stdout_to_close() {
    crate::support::test_support::with_fake_path(
            &[(
                "git",
                "#!/bin/sh\nif [ \"$1\" = \"log\" ]; then\n(sleep 1) &\nprintf 'abc123 message (1 day ago) <dev>\\nbody\\n---END---\\n'\nelse\nexit 9\nfi\n",
            )],
            || {
                let start = Instant::now();
                let output = run_log(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("abc123 message"));
                assert!(output.stdout.contains("body"));
                assert!(start.elapsed() < Duration::from_millis(700));
            },
        );
}
