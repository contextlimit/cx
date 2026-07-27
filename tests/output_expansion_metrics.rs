#![cfg(unix)]

use std::fs;

use cx::commands::{cmake_cmd, container, ctest_cmd, find, git, grep, ls, node_cmd, ps_cmd, read};
use cx::support::insights::TextMetrics;
use cx::support::runner::ProxyOutcome;

#[allow(dead_code)]
#[path = "../benches/support/mod.rs"]
mod bench_support;

use bench_support as support;

#[test]
fn test_and_syntax_success_summaries_have_small_expansion_ceilings() {
    let temp = support::ProjectTempDir::new("expansion-test-syntax");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    support::write_executable(&bin, "ctest", &support::output_script(None, None, 0));
    support::write_executable(&bin, "cmake", &support::output_script(None, None, 0));
    support::write_executable(&bin, "node", &support::output_script(None, None, 0));
    let js_path = temp.path().join("tiny.js");
    support::write_file(&js_path, "");
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let ctest = ctest_cmd::run(&[]).unwrap();
    assert_expansion_ceiling("ctest tiny success", &ctest, "test-result-summary", 4, 1);

    let cmake = cmake_cmd::run_build(&["build".to_string()]).unwrap();
    assert_expansion_ceiling("cmake tiny success", &cmake, "build-result-summary", 8, 1);

    let node = node_cmd::run(&strings(&["--check", js_path.to_str().unwrap()])).unwrap();
    assert_expansion_ceiling(
        "node check tiny success",
        &node,
        "syntax-check-summary",
        64,
        1,
    );
}

#[test]
fn no_match_and_clean_status_summaries_have_small_expansion_ceilings() {
    let temp = support::ProjectTempDir::new("expansion-search-status");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    support::write_executable(&bin, "rg", &support::output_script(None, None, 1));
    let status_path = temp.path().join("status.out");
    support::write_file(&status_path, "## main\n");
    support::write_executable(
        &bin,
        "git",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = status ]; then /bin/cat {}; exit 0; fi\nexit 9\n",
            support::shell_quote(&status_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let search = grep::run(
        "needle",
        &[temp.path().display().to_string()],
        &grep::GrepOptions::default(),
    )
    .unwrap();
    assert_eq!(search.exit_code, 1);
    assert_expansion_ceiling("grep no match", &search, "no-match-summary", 12, 1);

    let status = git::run_status(&[]).unwrap();
    assert_expansion_ceiling("git clean status", &status, "status-summary", 12, 1);
}

#[test]
fn discovery_and_read_formatting_have_small_expansion_ceilings() {
    let temp = support::ProjectTempDir::new("expansion-discovery-read");
    let empty_root = temp.path().join("empty");
    fs::create_dir_all(&empty_root).unwrap();

    let found = find::run(&strings(&[empty_root.to_str().unwrap(), "-type", "f"])).unwrap();
    assert_expansion_ceiling("find empty result", &found, "bounded-result-summary", 6, 1);

    let source_path = temp.path().join("tiny.txt");
    support::write_file(&source_path, "alpha\n");
    let read = read::run(
        &source_path,
        &read::ReadOptions {
            line_range: Some(read::ReadRange {
                start: Some(1),
                end: Some(1),
            }),
            line_numbers: true,
            auto_aggressive: false,
            ..read::ReadOptions::default()
        },
    )
    .unwrap();
    assert_expansion_ceiling("read line numbers", &read, "read-formatting", 8, 1);
}

#[test]
fn inventory_summaries_have_small_expansion_ceilings() {
    let temp = support::ProjectTempDir::new("expansion-inventory");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    support::write_executable(&bin, "ls", &support::output_script(None, None, 0));
    support::write_executable(
        &bin,
        "docker",
        "#!/bin/sh\nif [ \"$1\" = ps ]; then exit 0; fi\nexit 9\n",
    );
    support::write_executable(
        &bin,
        "ps",
        "#!/bin/sh\nprintf 'PID COMMAND\\n101 /usr/bin/node app.mjs\\n'\n",
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let listing = ls::run(&[]).unwrap();
    assert_expansion_ceiling("ls empty directory", &listing, "directory-summary", 5, 1);

    let containers = container::run_docker_ps(&[]).unwrap();
    assert_expansion_ceiling("docker ps empty", &containers, "container-summary", 8, 1);

    let processes = ps_cmd::run(&["-axo".to_string(), "pid,command".to_string()]).unwrap();
    assert_eq!(processes.stdout, "PID COMMAND\n101 /usr/bin/node app.mjs");
    let observation = processes.observation.as_ref().unwrap();
    let emitted = TextMetrics::from_text(&processes.stdout);
    assert_eq!(emitted.lines, observation.metrics.lines);
    assert_eq!(emitted.tokens, observation.metrics.tokens);
    assert_eq!(emitted.bytes + 1, observation.metrics.bytes);
    assert_eq!(emitted.chars + 1, observation.metrics.chars);
}

fn assert_expansion_ceiling(
    label: &str,
    outcome: &ProxyOutcome,
    expected_reason: &str,
    max_extra_tokens: u64,
    max_extra_lines: u64,
) {
    let observation = outcome
        .observation
        .as_ref()
        .unwrap_or_else(|| panic!("{label}: missing raw observation"));
    assert_eq!(
        observation.expansion_reason.as_deref(),
        Some(expected_reason),
        "{label}: wrong expansion reason"
    );
    let emitted = TextMetrics::from_text(&emitted_text(outcome));
    let extra_tokens = emitted.tokens.saturating_sub(observation.metrics.tokens);
    let extra_lines = emitted.lines.saturating_sub(observation.metrics.lines);
    let extra_chars = emitted.chars.saturating_sub(observation.metrics.chars);
    assert!(
        extra_tokens > 0 || extra_lines > 0 || extra_chars > 0,
        "{label}: fixture did not exercise expansion"
    );
    assert!(
        extra_tokens <= max_extra_tokens,
        "{label}: expanded by {extra_tokens} tokens, ceiling is {max_extra_tokens}"
    );
    assert!(
        extra_lines <= max_extra_lines,
        "{label}: expanded by {extra_lines} lines, ceiling is {max_extra_lines}"
    );
}

fn emitted_text(outcome: &ProxyOutcome) -> String {
    if outcome.stdout.is_empty() {
        outcome.stderr.clone()
    } else if outcome.stderr.is_empty() {
        outcome.stdout.clone()
    } else {
        format!("{}\n{}", outcome.stdout, outcome.stderr)
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
