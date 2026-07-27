#![cfg(unix)]

use std::ffi::OsString;
use std::process::Command;
use std::{env, fs};

use cx::commands::{
    cargo_cmd, cmake_cmd, container, ctest_cmd, find, git, go_cmd, grep, ls, node_cmd, ps_cmd,
    pytest_cmd, read, read_like, shell_cmd, tsc_cmd,
};
use cx::support::insights::TextMetrics;
use cx::support::runner::{run_filtered, RunOptions};

#[path = "../benches/support/mod.rs"]
mod bench_support;

use bench_support as support;

#[test]
fn plan_json_metrics_preserve_the_full_payload() {
    let temp = support::ProjectTempDir::new("metrics-plan-json");
    let plan_dir = temp.path().join(".state").join("plans/example");
    fs::create_dir_all(&plan_dir).unwrap();
    let plan_path = plan_dir.join("planSteps.json");
    let payload = format!(
        r#"{{"planSteps":[{{"body":"{}","status":"in_progress"}}]}}"#,
        "preserve-this-plan-requirement-".repeat(160)
    );
    support::write_file(&plan_path, &payload);

    let outcome = read_like::run_sed(&[
        "-n".to_string(),
        "1,$p".to_string(),
        plan_path.display().to_string(),
    ])
    .unwrap();

    assert_eq!(outcome.stdout, payload);
    assert_eq!(outcome.stdout.len(), payload.len());
    assert_eq!(outcome.stdout.lines().count(), payload.lines().count());
    assert!(serde_json::from_str::<serde_json::Value>(&outcome.stdout).is_ok());
}

#[test]
fn read_aggressive_metrics_reduce_large_source_and_keep_signatures() {
    let temp = support::ProjectTempDir::new("metrics-read");
    let source = support::rust_source_fixture(420);
    let source_path = temp.path().join("src/lib.rs");
    support::write_file(&source_path, &source);

    let outcome = read::run(
        &source_path,
        &read::ReadOptions {
            mode: read::ReadMode::Aggressive,
            max_lines: Some(90),
            auto_aggressive: false,
            ..read::ReadOptions::default()
        },
    )
    .unwrap();

    support::assert_reduction(
        "read aggressive metrics",
        &source,
        &outcome.stdout,
        0.35,
        &["bench_fn_0000", "pub fn"],
    );
}

#[test]
fn cat_read_like_metrics_reduce_large_source_and_keep_signatures() {
    let temp = support::ProjectTempDir::new("metrics-cat-read-like");
    let source = support::rust_source_fixture(900);
    let source_path = temp.path().join("src/lib.rs");
    support::write_file(&source_path, &source);

    let outcome = read_like::run_cat(&[source_path.display().to_string()]).unwrap();

    support::assert_reduction(
        "cat read-like metrics",
        &source,
        &outcome.stdout,
        0.35,
        &["bench_fn_0000", "pub fn"],
    );
}

#[test]
fn shell_generated_one_line_metrics_reduce_payload_and_keep_edge_evidence() {
    let temp = support::ProjectTempDir::new("metrics-shell-generated-line");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    let _env = support::EnvGuard::fake_path(&bin, &home);
    let raw = format!(
        r#"{{"status":"ok","payload":"{}","tail":"complete"}}"#,
        "A".repeat(40_000)
    );
    let output_path = temp.path().join("service-response.json");
    support::write_file(&output_path, &format!("{raw}\n"));
    let script = format!("/bin/cat {}", support::shell_quote(&output_path));
    let outcome = shell_cmd::run(&["-lc".to_string(), script]).unwrap();

    support::assert_reduction(
        "shell generated one-line metrics",
        &format!("{raw}\n"),
        &outcome.stdout,
        0.90,
        &[
            r#"{"status":"ok""#,
            "generated line truncated",
            r#""tail":"complete"}"#,
        ],
    );
}

#[test]
fn shell_no_compact_metrics_preserve_authoritative_output() {
    let temp = support::ProjectTempDir::new("metrics-shell-no-compact");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    let _env = support::EnvGuard::fake_path(&bin, &home);
    let raw = (1..=80)
        .map(|line| format!("record_{line:02}\n"))
        .collect::<String>();
    let script = "for i in {1..80}; do printf 'record_%02d\\n' \"$i\"; done";
    let outcome =
        shell_cmd::run_with_options(&["-lc".to_string(), script.to_string()], true).unwrap();

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, raw);
    assert_eq!(outcome.stdout.lines().count(), 80);
    assert!(!outcome.stdout.contains("lines omitted"));
    let observation = outcome
        .observation
        .expect("raw shell output should be observed");
    assert_eq!(observation.response.as_deref(), Some(raw.as_str()));
}

#[test]
fn shell_modest_output_metrics_preserve_authoritative_output() {
    let temp = support::ProjectTempDir::new("metrics-shell-modest");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    let _env = support::EnvGuard::fake_path(&bin, &home);
    let raw = (1..=103)
        .map(|line| format!("bounded_{line:03}\n"))
        .collect::<String>();
    let script = "for i in {1..103}; do printf 'bounded_%03d\\n' \"$i\"; done";
    let outcome = shell_cmd::run(&["-lc".to_string(), script.to_string()]).unwrap();

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, raw);
    assert!(!outcome.stdout.contains("lines omitted"));
    let observation = outcome
        .observation
        .expect("raw shell output should be observed");
    assert_eq!(observation.response.as_deref(), Some(raw.as_str()));
}

#[test]
fn shell_explicit_large_source_ranges_preserve_authoritative_output() {
    let temp = support::ProjectTempDir::new("metrics-shell-explicit-source-range");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    support::write_executable(&bin, "sed", "#!/bin/sh\nexec /usr/bin/sed \"$@\"\n");
    let _env = support::EnvGuard::fake_path(&bin, &home);
    let first = temp.path().join("first.cpp");
    let second = temp.path().join("second.cpp");
    let first_content = (1..=300)
        .map(|line| format!("int first_{line:04}();\n"))
        .collect::<String>();
    let second_content = (1..=2_600)
        .map(|line| format!("int second_{line:04}();\n"))
        .collect::<String>();
    support::write_file(&first, &first_content);
    support::write_file(&second, &second_content);
    let first_selected = first_content
        .lines()
        .take(260)
        .collect::<Vec<_>>()
        .join("\n");
    let second_selected = second_content
        .lines()
        .take(2_500)
        .collect::<Vec<_>>()
        .join("\n");
    let expected = format!("{first_selected}\n{second_selected}\n");
    let script = format!(
        "sed -n '1,260p' {}; sed -n '1,2500p' {}",
        support::shell_quote(&first),
        support::shell_quote(&second),
    );
    let outcome = shell_cmd::run(&["-lc".to_string(), script]).unwrap();

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, expected);
    assert_eq!(outcome.stdout.lines().count(), 2_760);
    assert!(!outcome.stdout.contains("lines omitted"));
    let observation = outcome
        .observation
        .expect("raw shell output should be observed");
    assert_eq!(observation.response.as_deref(), Some(expected.as_str()));
    assert_eq!(observation.metrics, TextMetrics::from_text(&expected));
}

#[test]
fn evidence_diff_metrics_preserve_raw_patch_byte_for_byte() {
    let temp = support::ProjectTempDir::new("metrics-evidence-diff");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = (0..160)
        .map(|line| format!("+exact_evidence_{line:03}\n"))
        .collect::<String>();
    let output_path = temp.path().join("evidence.diff");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "git",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = diff ]; then /bin/cat {}; exit 0; fi\nexit 9\n",
            support::shell_quote(&output_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = git::run_evidence_diff(&["HEAD~1..HEAD".to_string()]).unwrap();

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, raw);
    let observation = outcome.observation.unwrap();
    assert_eq!(observation.source, "git evidence-diff");
    assert_eq!(observation.response.as_deref(), Some(raw.as_str()));
}

#[test]
fn grep_metrics_reduce_many_matches_and_keep_file_line_evidence() {
    let temp = support::ProjectTempDir::new("metrics-grep");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::grep_output_fixture(512);
    let output_path = temp.path().join("rg.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "rg",
        &format!(
            "#!/bin/sh\ncase \" $* \" in\n  *\" nomatch \"*) exit 1 ;;\n  *) /bin/cat {} ;;\nesac\n",
            support::shell_quote(&output_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = grep::run_many(
        &["needle".to_string(), "route_state".to_string()],
        &["src".to_string()],
        &grep::GrepOptions {
            fixed_strings: true,
            max_results: Some(10),
            ..grep::GrepOptions::default()
        },
    )
    .unwrap();

    support::assert_reduction(
        "grep metrics",
        &raw,
        &outcome.stdout,
        0.45,
        &[
            "10 shown of 512 matches",
            "[file] src/module_00.rs",
            "+502 more matches hidden by --max-results",
        ],
    );
}

#[test]
fn grep_metrics_keep_small_results_exact_without_expansion() {
    let temp = support::ProjectTempDir::new("metrics-grep-small-exact");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = "330:        let rawBytes = metrics.raw_bytes;\n";
    let output_path = temp.path().join("rg.out");
    support::write_file(&output_path, raw);
    support::write_executable(
        &bin,
        "rg",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = grep::run(
        "rawBytes",
        &["src/archive.rs".to_string()],
        &grep::GrepOptions {
            fixed_strings: true,
            ..grep::GrepOptions::default()
        },
    )
    .unwrap();

    assert_eq!(outcome.stdout, raw.trim_end());
    let observation = outcome.observation.unwrap();
    assert_eq!(observation.response.as_deref(), Some(raw));
    assert!(outcome.stdout.len() <= observation.metrics.bytes as usize);
}

#[test]
fn grep_no_compact_metrics_preserve_large_native_output_exactly() {
    let temp = support::ProjectTempDir::new("metrics-grep-no-compact");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::grep_output_fixture(160);
    let output_path = temp.path().join("rg.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "rg",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = grep::run(
        "needle",
        &["src".to_string()],
        &grep::GrepOptions {
            no_compact: true,
            ..grep::GrepOptions::default()
        },
    )
    .unwrap();

    assert_eq!(outcome.stdout, raw);
    let observation = outcome.observation.unwrap();
    assert_eq!(observation.metrics.bytes as usize, raw.len());
    assert_eq!(observation.response.as_deref(), Some(raw.as_str()));
}

#[test]
fn grep_metrics_preserve_tsv_results_when_document_compaction_is_disabled() {
    let temp = support::ProjectTempDir::new("metrics-grep-tsv-exact");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = (1..=24)
        .map(|line| format!("audit/report.tsv:{line}:field_{line:02}\tvalue_{line:02}\n"))
        .collect::<String>();
    let output_path = temp.path().join("rg.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "rg",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = grep::run(
        "field",
        &["audit/report.tsv".to_string()],
        &grep::GrepOptions {
            fixed_strings: true,
            ..grep::GrepOptions::default()
        },
    )
    .unwrap();

    assert_eq!(outcome.stdout, raw.trim_end());
    assert!(!outcome.stdout.contains("[file]"));
    assert!(!outcome.stdout.contains("more in file"));
    let observation = outcome.observation.unwrap();
    assert_eq!(observation.metrics.bytes as usize, raw.len());
}

#[test]
fn grep_list_files_metrics_reduce_many_files_and_keep_paths() {
    let temp = support::ProjectTempDir::new("metrics-rg-files");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::file_list_fixture(180);
    let output_path = temp.path().join("rg-files.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "rg",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = grep::list_files(
        &["src".to_string()],
        &grep::GrepOptions {
            glob_patterns: vec!["*.rs".to_string()],
            max_results: Some(20),
            ..grep::GrepOptions::default()
        },
    )
    .unwrap();

    support::assert_reduction(
        "rg files metrics",
        &raw,
        &outcome.stdout,
        0.35,
        &[
            "20 shown of 180 files",
            "src/module_000.rs",
            "+160 more files hidden by --max-results",
        ],
    );
}

#[test]
fn grep_fallback_metrics_reduce_many_matches_and_keep_evidence() {
    let temp = support::ProjectTempDir::new("metrics-grep-fallback");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::grep_output_fixture(120);
    let output_path = temp.path().join("grep.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "grep",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = grep::run_many(
        &["needle".to_string(), "route_state".to_string()],
        &["src".to_string()],
        &grep::GrepOptions {
            fixed_strings: true,
            max_results: Some(10),
            ..grep::GrepOptions::default()
        },
    )
    .unwrap();

    support::assert_reduction(
        "grep fallback metrics",
        &raw,
        &outcome.stdout,
        0.45,
        &[
            "10 shown of 120 matches",
            "[file] src/module_00.rs",
            "+110 more matches hidden by --max-results",
        ],
    );
}

#[test]
fn ls_metrics_reduce_directory_listing_and_keep_entries() {
    let temp = support::ProjectTempDir::new("metrics-ls");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::ls_output_fixture(180);
    let output_path = temp.path().join("ls.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "ls",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = ls::run(&[]).unwrap();

    support::assert_reduction(
        "ls metrics",
        &raw,
        &outcome.stdout,
        0.35,
        &["src/", "file_000.rs", "Summary:"],
    );
}

#[test]
fn docker_ps_metrics_reduce_container_listing_and_keep_names() {
    let temp = support::ProjectTempDir::new("metrics-docker-ps");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::docker_ps_output_fixture(60);
    let output_path = temp.path().join("docker-ps.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "docker",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"ps\" ]; then\n/bin/cat {}\nelse\nexit 1\nfi\n",
            support::shell_quote(&output_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = container::run_docker_ps(&[]).unwrap();

    support::assert_reduction(
        "docker ps metrics",
        &raw,
        &outcome.stdout,
        0.35,
        &["[docker] 60 containers", "web_000", "... +45 more"],
    );
}

#[test]
fn git_diff_metrics_reduce_large_diff_and_keep_summary() {
    let temp = support::ProjectTempDir::new("metrics-git-diff");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let stat = support::git_diff_stat_fixture();
    let raw_diff = support::git_diff_fixture(240);
    let stat_path = temp.path().join("git-diff-stat.out");
    let diff_path = temp.path().join("git-diff.out");
    support::write_file(&stat_path, &stat);
    support::write_file(&diff_path, &raw_diff);
    support::write_executable(
        &bin,
        "git",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"diff\" ] && [ \"$2\" = \"--stat\" ]; then\n/bin/cat {}\nelif [ \"$1\" = \"diff\" ]; then\n/bin/cat {}\nelse\nexit 9\nfi\n",
            support::shell_quote(&stat_path),
            support::shell_quote(&diff_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = git::run_diff(&[]).unwrap();

    support::assert_reduction(
        "git diff metrics",
        &raw_diff,
        &outcome.stdout,
        0.45,
        &["src/lib.rs", "+240 -240", "[full diff:"],
    );
}

#[test]
fn git_status_and_log_metrics_keep_worktree_and_history_evidence() {
    let temp = support::ProjectTempDir::new("metrics-git-status-log");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let status_raw = large_git_status_fixture();
    let log_raw = support::git_log_fixture(40);
    let status_path = temp.path().join("git-status.out");
    let log_path = temp.path().join("git-log.out");
    support::write_file(&status_path, &status_raw);
    support::write_file(&log_path, &log_raw);
    support::write_executable(
        &bin,
        "git",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"status\" ]; then\n/bin/cat {}\nelif [ \"$1\" = \"log\" ]; then\n/bin/cat {}\nelse\nexit 9\nfi\n",
            support::shell_quote(&status_path),
            support::shell_quote(&log_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let status = git::run_status(&[]).unwrap();
    support::assert_reduction(
        "git status metrics",
        &status_raw,
        &status.stdout,
        0.35,
        &["* main", "+ Staged: 90 files", "? Untracked: 90 files"],
    );

    let log = git::run_log(&[]).unwrap();
    support::assert_reduction(
        "git log metrics",
        &log_raw,
        &log.stdout,
        0.35,
        &["abc000 r0 Improve benchmark path", "[+1 lines omitted]"],
    );
    assert!(!log.stdout.contains("abc039"));
}

#[test]
fn runner_fallback_metrics_reduce_unparsed_large_output() {
    let temp = support::ProjectTempDir::new("metrics-runner");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = numbered_lines("stdout line", 700);
    let output_path = temp.path().join("large.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "large-output",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = run_filtered(
        Command::new("large-output"),
        "large-output",
        |_| None::<String>,
        RunOptions::default().fallback_window(8, 8),
    )
    .unwrap();

    support::assert_reduction(
        "runner fallback metrics",
        &raw,
        &outcome.stdout,
        0.08,
        &["stdout line 0000", "stdout line 0699"],
    );
}

#[test]
fn wrapper_fixture_metrics_reduce_noisy_test_outputs_and_keep_evidence() {
    let temp = support::ProjectTempDir::new("metrics-wrappers");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let cargo_raw = support::cargo_compile_failure_fixture(140);
    let pytest_raw = support::pytest_failure_fixture(120);
    let go_raw = support::go_json_failure_fixture(180);
    let ctest_raw = support::ctest_failure_fixture(160);
    let cargo_out = temp.path().join("cargo.out");
    let pytest_out = temp.path().join("pytest.out");
    let go_out = temp.path().join("go.out");
    let ctest_out = temp.path().join("ctest.out");
    support::write_file(&cargo_out, &cargo_raw);
    support::write_file(&pytest_out, &pytest_raw);
    support::write_file(&go_out, &go_raw);
    support::write_file(&ctest_out, &ctest_raw);
    support::write_executable(
        &bin,
        "cargo",
        &support::output_script(Some(&cargo_out), None, 101),
    );
    support::write_executable(
        &bin,
        "pytest",
        &support::output_script(Some(&pytest_out), None, 1),
    );
    support::write_executable(&bin, "go", &support::output_script(Some(&go_out), None, 1));
    support::write_executable(
        &bin,
        "ctest",
        &support::output_script(Some(&ctest_out), None, 8),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let cargo = cargo_cmd::run_test(&[]).unwrap();
    support::assert_reduction(
        "cargo metrics",
        &cargo_raw,
        &cargo.stdout,
        0.30,
        &["E0425", "src/lib.rs:42:9", "could not compile"],
    );

    let pytest = pytest_cmd::run(&[]).unwrap();
    support::assert_reduction(
        "pytest metrics",
        &pytest_raw,
        &pytest.stdout,
        0.20,
        &["Pytest: 119 passed, 1 failed", "test_alpha"],
    );

    let go = go_cmd::run_test(&[]).unwrap();
    support::assert_reduction(
        "go metrics",
        &go_raw,
        &go.stdout,
        0.20,
        &["Go test:", "TestWidget", "expected 5, got 3"],
    );

    let ctest = ctest_cmd::run(&[]).unwrap();
    support::assert_reduction(
        "ctest metrics",
        &ctest_raw,
        &ctest.stdout,
        0.20,
        &["ctest: failed", "sample-ui-dockview-perf-mouse-e2e"],
    );
}

#[test]
fn ctest_list_metrics_reduce_large_catalog_and_keep_catalog_evidence() {
    let temp = support::ProjectTempDir::new("metrics-ctest-list");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::ctest_list_fixture(180);
    let output_path = temp.path().join("ctest-list.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "ctest",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let ctest = ctest_cmd::run(&[
        "--test-dir".to_string(),
        "build-web".to_string(),
        "-N".to_string(),
        "-R".to_string(),
        "sample-ui-catalog-model-test".to_string(),
    ])
    .unwrap();

    support::assert_reduction(
        "ctest list metrics",
        &raw,
        &ctest.stdout,
        0.85,
        &[
            "ctest: list",
            "sample-ui-catalog-model-test-000",
            "sample-ui-catalog-model-test-179",
            "Total Tests: 180",
            "lines omitted",
        ],
    );
    assert!(!ctest.stdout.contains("ctest: ok"));
}

#[test]
fn build_wrapper_metrics_reduce_noisy_outputs_and_keep_diagnostics() {
    let temp = support::ProjectTempDir::new("metrics-build-wrappers");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let cmake_raw = support::cmake_failure_fixture(140);
    let tsc_raw = support::tsc_failure_fixture(120);
    let cmake_out = temp.path().join("cmake.out");
    let tsc_out = temp.path().join("tsc.out");
    support::write_file(&cmake_out, &cmake_raw);
    support::write_file(&tsc_out, &tsc_raw);
    support::write_executable(
        &bin,
        "cmake",
        &support::output_script(Some(&cmake_out), None, 2),
    );
    support::write_executable(
        &bin,
        "tsc",
        &support::output_script(Some(&tsc_out), None, 2),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let cmake = cmake_cmd::run_build(&[
        "build".to_string(),
        "--target".to_string(),
        "cx".to_string(),
    ])
    .unwrap();
    support::assert_reduction(
        "cmake metrics",
        &cmake_raw,
        &cmake.stdout,
        0.45,
        &[
            "cmake build: failed",
            "missing_runtime.cpp",
            "ninja: build stopped",
        ],
    );

    let tsc = tsc_cmd::run(&[
        "--noEmit".to_string(),
        "--project".to_string(),
        "tsconfig.json".to_string(),
    ])
    .unwrap();
    support::assert_reduction(
        "tsc metrics",
        &tsc_raw,
        &tsc.stdout,
        0.45,
        &["TypeScript: 3 errors", "TS2322", "src/view.tsx"],
    );
}

#[test]
fn cmake_success_metrics_reduce_progress_and_keep_target_evidence() {
    let temp = support::ProjectTempDir::new("metrics-cmake-success");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::cmake_success_fixture(180);
    let output_path = temp.path().join("cmake-success.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "cmake",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = cmake_cmd::run_build(&[
        "build".to_string(),
        "--target".to_string(),
        "cx".to_string(),
    ])
    .unwrap();
    support::assert_reduction(
        "cmake success metrics",
        &raw,
        &outcome.stdout,
        0.45,
        &["cmake build: ok", "Built target cx", "lines omitted"],
    );
}

#[test]
fn log_wrapper_metrics_reduce_noisy_logs_and_keep_error_evidence() {
    let temp = support::ProjectTempDir::new("metrics-log-wrappers");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::log_output_fixture(180);
    let output_path = temp.path().join("logs.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "docker",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"logs\" ]; then\n/bin/cat {}\nelse\nexit 1\nfi\n",
            support::shell_quote(&output_path)
        ),
    );
    support::write_executable(
        &bin,
        "kubectl",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"logs\" ]; then\n/bin/cat {}\nelse\nexit 1\nfi\n",
            support::shell_quote(&output_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let docker = container::run_docker_logs("web", &[]).unwrap();
    support::assert_reduction(
        "docker logs metrics",
        &raw,
        &docker.stdout,
        0.30,
        &["[docker] Logs for web", "Log Summary", "[ERRORS]"],
    );

    let kubectl =
        container::run_kubectl_logs("pod-1", &["--namespace".to_string(), "default".to_string()])
            .unwrap();
    support::assert_reduction(
        "kubectl logs metrics",
        &raw,
        &kubectl.stdout,
        0.30,
        &["Logs for pod-1", "Log Summary", "[WARNINGS]"],
    );
}

#[test]
fn ps_inventory_metrics_reduce_broad_tables_and_keep_process_evidence() {
    let temp = support::ProjectTempDir::new("metrics-ps-inventory");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::ps_output_fixture(360);
    let output_path = temp.path().join("ps.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "ps",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = ps_cmd::run(&["-axo".to_string(), "pid,ppid,etime,command".to_string()]).unwrap();
    support::assert_reduction(
        "ps inventory metrics",
        &raw,
        &outcome.stdout,
        0.12,
        &[
            "ps: 360 processes across 3 executables",
            "node |",
            "python3 |",
            "rare-service | 1",
            "[REDACTED]",
            "[full process table: cx -- ps --no-compact",
        ],
    );
    assert_eq!(
        outcome
            .observation
            .as_ref()
            .and_then(|observation| observation.expansion_reason.as_deref()),
        Some("process-inventory-summary")
    );
}

#[test]
fn find_metrics_reduce_large_tree_and_keep_observed_predicates() {
    let temp = support::ProjectTempDir::new("metrics-find");
    let fixture = support::populate_find_fixture(temp.path(), 360);
    let root = fixture.root.display().to_string();

    let name_out = find::run(&strings(&[
        &root,
        "-maxdepth",
        "4",
        "-type",
        "f",
        "-name",
        "*.rs",
        "--max-results",
        "30",
    ]))
    .unwrap();
    support::assert_reduction(
        "find name metrics",
        &fixture.raw_rs_listing,
        &name_out.stdout,
        0.15,
        &[
            "30 shown of 360 entries",
            "file_0000.rs",
            "... +330 more entries hidden by --max-results",
        ],
    );

    let path_out = find::run(&strings(&[
        &root,
        "-maxdepth",
        "5",
        "-type",
        "d",
        "-path",
        "*/node_modules/playwright",
    ]))
    .unwrap();
    assert_eq!(path_out.exit_code, 0);
    assert!(path_out.stdout.contains(&fixture.path_dir_name));

    let perm_out = find::run(&strings(&[&root, "-type", "f", "-perm", "-111"])).unwrap();
    assert_eq!(perm_out.exit_code, 0);
    assert!(perm_out.stdout.contains(&fixture.executable_name));
}

#[test]
fn node_check_metrics_preserve_delegated_truth_and_jsx_path() {
    let temp = support::ProjectTempDir::new("metrics-node-check");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    support::write_executable(
        &bin,
        "node",
        support::node_import_assertion_failure_script(),
    );
    let fixture = support::write_node_check_fixtures(temp.path());
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let js = node_cmd::run(&strings(&[
        "--check",
        &fixture.js_file.display().to_string(),
    ]))
    .unwrap();
    assert_eq!(js.exit_code, 0);
    assert!(js.stdout.contains("plain.js"));
    assert!(js.stderr.is_empty());

    let jsx = node_cmd::run(&strings(&[
        "--check",
        &fixture.jsx_file.display().to_string(),
    ]))
    .unwrap();
    assert_eq!(jsx.exit_code, 0);
    assert!(jsx.stdout.contains("[jsx parser]"));
    assert!(jsx.stderr.is_empty());

    let mjs = node_cmd::run(&strings(&[
        "--check",
        &fixture.mjs_file.display().to_string(),
    ]))
    .unwrap();
    assert_eq!(mjs.exit_code, 1);
    assert!(mjs.stdout.is_empty());
    assert!(mjs.stderr.contains("native_core.mjs"));
    assert!(mjs.stderr.contains("SyntaxError"));
    assert!(mjs.stderr.contains("'assert'"));

    let mixed = node_cmd::run(&strings(&[
        "--check",
        &fixture.js_file.display().to_string(),
        &fixture.jsx_file.display().to_string(),
        &fixture.mjs_file.display().to_string(),
    ]))
    .unwrap();
    assert_eq!(mixed.exit_code, 1);
    assert!(mixed.stdout.contains("plain.js"));
    assert!(mixed.stdout.contains("[jsx parser]"));
    assert!(mixed.stderr.contains("native_core.mjs"));
    assert!(mixed.stderr.contains("Unexpected identifier"));

    let large_jsx = (0..260)
        .map(|index| format!("const value_{index:03} = {index};\n"))
        .chain(std::iter::once(
            "export default function Panel() { return <section data-ok />; }\n".to_string(),
        ))
        .collect::<String>();
    let large_jsx_path = temp.path().join("large.jsx");
    support::write_file(&large_jsx_path, &large_jsx);
    let large = node_cmd::run(&strings(&[
        "--check",
        &large_jsx_path.display().to_string(),
    ]))
    .unwrap();
    support::assert_reduction(
        "node check large jsx metrics",
        &large_jsx,
        &large.stdout,
        0.10,
        &["node --check: syntax ok", "[jsx parser]"],
    );
}

#[test]
fn node_test_metrics_reduce_long_tap_output_and_keep_raw_counts() {
    let temp = support::ProjectTempDir::new("metrics-node-test");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    support::write_executable(
        &bin,
        "node",
        "#!/bin/sh\nif [ \"$1\" = \"--test\" ]; then\nprintf 'TAP version 13\\n'\ni=1\nwhile [ \"$i\" -le 100 ]; do printf 'ok %s - test %s\\n' \"$i\" \"$i\"; i=$((i + 1)); done\nprintf '1..100\\n# tests 100\\n# pass 100\\n# fail 0\\n# duration_ms 20\\n'\nexit 0\nfi\nprintf 'unexpected invocation\\n' >&2\nexit 9\n",
    );
    fs::create_dir_all(&home).unwrap();
    let db_path = temp.path().join("node-run-insights.sqlite");
    let db_path_string = db_path.to_string_lossy().to_string();
    let _env = support::EnvGuard::fake_path(&bin, &home);
    let _db = ScopedEnvVar::set("CX_INSIGHTS_DB_PATH", db_path_string.as_str());

    let mut raw = String::from("TAP version 13\n");
    for index in 1..=100 {
        raw.push_str(&format!("ok {index} - test {index}\n"));
    }
    raw.push_str("1..100\n# tests 100\n# pass 100\n# fail 0\n# duration_ms 20\n");
    let outcome = node_cmd::run(&strings(&["test", "suite.test.mjs"])).unwrap();

    assert_eq!(outcome.exit_code, 0);
    support::assert_reduction(
        "node test metrics",
        &raw,
        &outcome.stdout,
        0.55,
        &[
            "TAP version 13",
            "ok 1 - test 1",
            "ok 100 - test 100",
            "# tests 100",
            "lines omitted",
        ],
    );
    assert!(outcome
        .observation
        .as_ref()
        .is_some_and(|raw| raw.source == "node test" && raw.metrics.lines == 106));
}

struct ScopedEnvVar {
    key: &'static str,
    original: Option<OsString>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: &str) -> Self {
        let original = env::var_os(key);
        env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => env::set_var(self.key, value),
            None => env::remove_var(self.key),
        }
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn numbered_lines(label: &str, count: usize) -> String {
    let mut output = String::new();
    for index in 0..count {
        output.push_str(&format!("{label} {index:04}\n"));
    }
    output
}

fn large_git_status_fixture() -> String {
    let mut output = support::git_status_porcelain_fixture();
    for index in 1..90 {
        output.push_str(&format!("M  src/module_{index:03}.rs\n"));
    }
    for index in 1..90 {
        output.push_str(&format!("?? generated/artifact_{index:03}.json\n"));
    }
    output
}
