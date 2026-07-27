#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use cx::commands::{
    cargo_cmd, cmake_cmd, container, ctest_cmd, git, go_cmd, grep, insights as insights_cmd, ls,
    node_cmd, passthrough, ps_cmd, pytest_cmd, tsc_cmd,
};
use cx::support::insights::{self as insight_store, InvocationRecord, TextMetrics};
use cx::support::runner::ProxyOutcome;

#[derive(Debug)]
struct FailureCase {
    command: &'static str,
    outcome: ProxyOutcome,
    exit_code: i32,
    artifact_tool: &'static str,
    artifact_marker: &'static str,
}

#[test]
fn failure_artifact_matrix_correlates_with_insights_failures() {
    let temp = ProjectTempDir::new("failure-artifact-matrix");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_failure_tools(&bin);

    let broken_jsx = temp.path().join("broken.jsx");
    write_file(
        &broken_jsx,
        "export default function Widget() { return <div>; }\n",
    );
    let broken_jsx_arg = broken_jsx.display().to_string();

    let _env = EnvGuard::fake_path(&bin, &home);
    let cases = vec![
        FailureCase {
            command: "git diff",
            outcome: git::run_diff(&[]).unwrap(),
            exit_code: 129,
            artifact_tool: "git",
            artifact_marker: "fatal: diff driver failed",
        },
        FailureCase {
            command: "node check",
            outcome: node_cmd::run(&strings(&["--check", &broken_jsx_arg])).unwrap(),
            exit_code: 1,
            artifact_tool: "node",
            artifact_marker: "broken.jsx",
        },
        FailureCase {
            command: "grep",
            outcome: grep::run_many(
                &["needle".to_string()],
                &["src".to_string()],
                &grep::GrepOptions::default(),
            )
            .unwrap(),
            exit_code: 2,
            artifact_tool: "grep",
            artifact_marker: "rg: error: fixture search failed",
        },
        FailureCase {
            command: "ls",
            outcome: ls::run(&["missing".to_string()]).unwrap(),
            exit_code: 2,
            artifact_tool: "ls",
            artifact_marker: "ls: cannot access",
        },
        FailureCase {
            command: "docker ps",
            outcome: container::run_docker_ps(&[]).unwrap(),
            exit_code: 125,
            artifact_tool: "docker",
            artifact_marker: "Cannot connect to the Docker daemon",
        },
    ];

    for case in &cases {
        assert_eq!(case.outcome.exit_code, case.exit_code, "{case:?}");
        assert_expansion_reason(case, expected_expansion_reason(case.command));
        record_case(case.command, &case.outcome);
        assert_artifact(&home, case.artifact_tool, case.artifact_marker);
    }

    let failures = insights_cmd::run_failures(
        10,
        insight_store::CommandLevel::Command,
        insight_store::CommandFilter::default(),
    )
    .unwrap();
    for case in &cases {
        assert_failure_focus_line(&failures.stdout, case.command, case.artifact_tool);
    }

    assert_eq!(
        insights_cmd::run_failures(
            10,
            insight_store::CommandLevel::Command,
            insight_store::CommandFilter::default(),
        )
        .unwrap()
        .exit_code,
        0
    );
    assert_insights_commands_are_not_recorded(&home);
}

#[test]
fn run_filtered_failure_matrix_correlates_with_insights_failures() {
    let temp = ProjectTempDir::new("run-filtered-failure-matrix");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_run_filtered_failure_tools(&bin);

    let _env = EnvGuard::fake_path(&bin, &home);
    let cases = vec![
        FailureCase {
            command: "cargo test",
            outcome: cargo_cmd::run_test(&[]).unwrap(),
            exit_code: 101,
            artifact_tool: "cargo",
            artifact_marker: "error[E0425]",
        },
        FailureCase {
            command: "pytest",
            outcome: pytest_cmd::run(&[]).unwrap(),
            exit_code: 1,
            artifact_tool: "pytest",
            artifact_marker: "FAILED test_sample.py::test_failure",
        },
        FailureCase {
            command: "go test",
            outcome: go_cmd::run_test(&[]).unwrap(),
            exit_code: 1,
            artifact_tool: "go",
            artifact_marker: "TestBroken",
        },
        FailureCase {
            command: "tsc",
            outcome: tsc_cmd::run(&[]).unwrap(),
            exit_code: 2,
            artifact_tool: "tsc",
            artifact_marker: "TS2322",
        },
        FailureCase {
            command: "cmake build",
            outcome: cmake_cmd::run_build(&["build".to_string()]).unwrap(),
            exit_code: 2,
            artifact_tool: "cmake",
            artifact_marker: "ninja: build stopped",
        },
        FailureCase {
            command: "ctest",
            outcome: ctest_cmd::run(&[]).unwrap(),
            exit_code: 8,
            artifact_tool: "ctest",
            artifact_marker: "The following tests FAILED",
        },
        FailureCase {
            command: "docker logs",
            outcome: container::run_docker_logs("web", &[]).unwrap(),
            exit_code: 125,
            artifact_tool: "docker",
            artifact_marker: "docker logs unavailable",
        },
        FailureCase {
            command: "kubectl logs",
            outcome: container::run_kubectl_logs("pod/web", &[]).unwrap(),
            exit_code: 1,
            artifact_tool: "kubectl",
            artifact_marker: "pods \"pod/web\" not found",
        },
    ];

    for case in &cases {
        assert_eq!(case.outcome.exit_code, case.exit_code, "{case:?}");
        record_case(case.command, &case.outcome);
        assert_artifact(&home, case.artifact_tool, case.artifact_marker);
    }

    let failures = insights_cmd::run_failures(
        20,
        insight_store::CommandLevel::Command,
        insight_store::CommandFilter::default(),
    )
    .unwrap();
    for case in &cases {
        assert_failure_focus_line(&failures.stdout, case.command, case.artifact_tool);
    }

    assert_eq!(
        insights_cmd::run_failures(
            20,
            insight_store::CommandLevel::Command,
            insight_store::CommandFilter::default(),
        )
        .unwrap()
        .exit_code,
        0
    );
    assert_insights_commands_are_not_recorded(&home);
}

#[test]
fn direct_capture_branch_matrix_correlates_with_insights_failures() {
    let temp = ProjectTempDir::new("direct-capture-branch-matrix");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_direct_capture_branch_failure_tools(&bin);

    let _env = EnvGuard::fake_path(&bin, &home);
    let cases = direct_capture_failure_cases();

    for case in &cases {
        assert_eq!(case.outcome.exit_code, case.exit_code, "{case:?}");
        assert_expansion_reason(case, expected_expansion_reason(case.command));
        record_case(case.command, &case.outcome);
    }
    assert_recorded_expansion_reason(
        &home,
        "passthrough demo-fail",
        "failure-artifact-recovery-hint",
    );

    let expected_artifact_counts = expected_artifact_counts(&cases);
    for case in &cases {
        assert_artifact_count_and_marker(
            &home,
            case.artifact_tool,
            expected_artifact_counts[case.artifact_tool],
            case.artifact_marker,
        );
    }

    let failures = insights_cmd::run_failures(
        20,
        insight_store::CommandLevel::Command,
        insight_store::CommandFilter::default(),
    )
    .unwrap();
    for case in &cases {
        assert_failure_focus_line_with_artifact_count(
            &failures.stdout,
            case.command,
            case.artifact_tool,
            expected_artifact_counts[case.artifact_tool],
        );
    }

    assert_eq!(
        insights_cmd::run_failures(
            20,
            insight_store::CommandLevel::Command,
            insight_store::CommandFilter::default(),
        )
        .unwrap()
        .exit_code,
        0
    );
    assert_insights_commands_are_not_recorded(&home);
}

fn direct_capture_failure_cases() -> Vec<FailureCase> {
    vec![
        FailureCase {
            command: "git status",
            outcome: git::run_status(&[]).unwrap(),
            exit_code: 128,
            artifact_tool: "git",
            artifact_marker: "fatal: status failed",
        },
        FailureCase {
            command: "git log",
            outcome: git::run_log(&[]).unwrap(),
            exit_code: 128,
            artifact_tool: "git",
            artifact_marker: "fatal: log failed",
        },
        FailureCase {
            command: "grep files",
            outcome: grep::list_files(&["src".to_string()], &grep::GrepOptions::default()).unwrap(),
            exit_code: 2,
            artifact_tool: "grep",
            artifact_marker: "rg --files unavailable",
        },
        FailureCase {
            command: "grep",
            outcome: grep::run_many(
                &["fallback".to_string()],
                &["src".to_string()],
                &grep::GrepOptions::default(),
            )
            .unwrap(),
            exit_code: 2,
            artifact_tool: "grep",
            artifact_marker: "grep fallback exploded",
        },
        FailureCase {
            command: "node check",
            outcome: node_cmd::run(&strings(&["--check", "broken.js"])).unwrap(),
            exit_code: 1,
            artifact_tool: "node",
            artifact_marker: "broken.js: SyntaxError",
        },
        FailureCase {
            command: "ps",
            outcome: ps_cmd::run(&strings(&["-p", "999"])).unwrap(),
            exit_code: 1,
            artifact_tool: "ps",
            artifact_marker: "ps: process 999 not found",
        },
        FailureCase {
            command: "passthrough demo-fail",
            outcome: passthrough::run(&strings(&["demo-fail", "alpha"])).unwrap(),
            exit_code: 7,
            artifact_tool: "passthrough",
            artifact_marker: "demo-fail: native stderr",
        },
    ]
}

fn write_failure_tools(bin: &Path) {
    write_executable(
        bin,
        "git",
        "#!/bin/sh\nif [ \"$1\" = \"diff\" ] && [ \"$2\" = \"--stat\" ]; then\nprintf ' src/lib.rs | 1 +\\n 1 file changed, 1 insertion(+)\\n'\nexit 0\nfi\nif [ \"$1\" = \"diff\" ]; then\nprintf 'fatal: diff driver failed\\n' >&2\nexit 129\nfi\nprintf 'unexpected git args: %s\\n' \"$*\" >&2\nexit 7\n",
    );
    write_executable(
        bin,
        "rg",
        "#!/bin/sh\nprintf 'rg: error: fixture search failed\\n' >&2\nexit 2\n",
    );
    write_executable(
        bin,
        "ls",
        "#!/bin/sh\nprintf 'ls: cannot access %s: No such file or directory\\n' \"$*\" >&2\nexit 2\n",
    );
    write_executable(
        bin,
        "docker",
        "#!/bin/sh\nif [ \"$1\" = \"ps\" ]; then\nprintf 'Cannot connect to the Docker daemon\\n' >&2\nexit 125\nfi\nprintf 'unexpected docker args: %s\\n' \"$*\" >&2\nexit 7\n",
    );
}

fn write_direct_capture_branch_failure_tools(bin: &Path) {
    write_executable(
        bin,
        "git",
        "#!/bin/sh\nif [ \"$1\" = \"status\" ]; then\nprintf 'fatal: status failed\\n' >&2\nexit 128\nfi\nif [ \"$1\" = \"log\" ]; then\nprintf 'fatal: log failed\\n' >&2\nexit 128\nfi\nprintf 'unexpected git args: %s\\n' \"$*\" >&2\nexit 7\n",
    );
    write_executable(
        bin,
        "rg",
        "#!/bin/sh\nif [ \"$1\" = \"--files\" ]; then\nprintf 'rg --files unavailable\\n' >&2\nexit 2\nfi\nprintf 'regex parse error: forced fallback\\n' >&2\nexit 2\n",
    );
    write_executable(
        bin,
        "grep",
        "#!/bin/sh\nprintf 'grep fallback exploded\\n' >&2\nexit 2\n",
    );
    write_executable(
        bin,
        "node",
        "#!/bin/sh\nif [ \"$1\" = \"--check\" ]; then\nprintf '%s: SyntaxError: expected ;\\n' \"$2\" >&2\nexit 1\nfi\nprintf 'unexpected node args: %s\\n' \"$*\" >&2\nexit 7\n",
    );
    write_executable(
        bin,
        "ps",
        "#!/bin/sh\nprintf 'ps: process 999 not found\\n' >&2\nexit 1\n",
    );
    write_executable(
        bin,
        "demo-fail",
        "#!/bin/sh\nprintf 'demo-fail: partial stdout\\n'\nprintf 'demo-fail: native stderr\\n' >&2\nexit 7\n",
    );
}

fn write_run_filtered_failure_tools(bin: &Path) {
    write_executable(
        bin,
        "cargo",
        "#!/bin/sh\nprintf 'error[E0425]: cannot find value `missing` in this scope\\n  --> src/lib.rs:1:1\\n'\nprintf 'error: could not compile fixture\\n'\nexit 101\n",
    );
    write_executable(
        bin,
        "pytest",
        "#!/bin/sh\nprintf '============================= FAILURES =============================\\n'\nprintf '____________________________ test_failure ___________________________\\n'\nprintf 'FAILED test_sample.py::test_failure - AssertionError: boom\\n'\nprintf '=========================== short test summary info ===========================\\n'\nprintf '1 failed in 0.01s\\n'\nexit 1\n",
    );
    write_executable(
        bin,
        "go",
        "#!/bin/sh\nprintf '{\"Action\":\"run\",\"Package\":\"example.com/app\",\"Test\":\"TestBroken\"}\\n'\nprintf '{\"Action\":\"output\",\"Package\":\"example.com/app\",\"Test\":\"TestBroken\",\"Output\":\"boom\\\\n\"}\\n'\nprintf '{\"Action\":\"fail\",\"Package\":\"example.com/app\",\"Test\":\"TestBroken\"}\\n'\nprintf '{\"Action\":\"fail\",\"Package\":\"example.com/app\"}\\n'\nexit 1\n",
    );
    write_executable(
        bin,
        "tsc",
        "#!/bin/sh\nprintf 'src/app.ts(1,7): error TS2322: Type string is not assignable to type number.\\n'\nexit 2\n",
    );
    write_executable(
        bin,
        "cmake",
        "#!/bin/sh\nprintf 'FAILED: app\\n' >&2\nprintf 'ninja: build stopped: subcommand failed.\\n' >&2\nexit 2\n",
    );
    write_executable(
        bin,
        "ctest",
        "#!/bin/sh\nprintf 'The following tests FAILED:\\n'\nprintf '  1 - fixture_test (Failed)\\n'\nexit 8\n",
    );
    write_executable(
        bin,
        "docker",
        "#!/bin/sh\nif [ \"$1\" = \"logs\" ]; then\nprintf 'docker logs unavailable\\n' >&2\nexit 125\nfi\nprintf 'unexpected docker args: %s\\n' \"$*\" >&2\nexit 7\n",
    );
    write_executable(
        bin,
        "kubectl",
        "#!/bin/sh\nif [ \"$1\" = \"logs\" ]; then\nprintf 'Error from server (NotFound): pods \"pod/web\" not found\\n' >&2\nexit 1\nfi\nprintf 'unexpected kubectl args: %s\\n' \"$*\" >&2\nexit 7\n",
    );
}

fn record_case(command: &str, outcome: &ProxyOutcome) {
    let record = InvocationRecord {
        command,
        exit_code: outcome.exit_code,
        raw: outcome.observation.as_ref(),
        emitted: emitted_metrics(outcome),
    };
    insight_store::record_invocation(&record).unwrap();
}

fn emitted_metrics(outcome: &ProxyOutcome) -> TextMetrics {
    let emitted = if outcome.stdout.is_empty() {
        outcome.stderr.clone()
    } else if outcome.stderr.is_empty() {
        outcome.stdout.clone()
    } else {
        format!("{}\n{}", outcome.stdout, outcome.stderr)
    };
    TextMetrics::from_text(&emitted)
}

fn assert_artifact(home: &Path, tool: &str, marker: &str) {
    let artifact_dir = home.join(".cx/cache/failures").join(tool);
    let artifacts = fs::read_dir(&artifact_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", artifact_dir.display()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(artifacts.len(), 1, "{tool} artifacts");
    let body = fs::read_to_string(artifacts[0].path()).unwrap();
    assert!(
        body.contains(marker),
        "{tool} artifact missing `{marker}` in\n{body}"
    );
}

fn assert_artifact_count_and_marker(home: &Path, tool: &str, expected_count: usize, marker: &str) {
    let artifact_dir = home.join(".cx/cache/failures").join(tool);
    let artifacts = fs::read_dir(&artifact_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", artifact_dir.display()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(artifacts.len(), expected_count, "{tool} artifacts");
    let bodies = artifacts
        .iter()
        .map(|entry| fs::read_to_string(entry.path()).unwrap())
        .collect::<Vec<_>>();
    assert!(
        bodies.iter().any(|body| body.contains(marker)),
        "{tool} artifacts missing `{marker}` in\n{}",
        bodies.join("\n--- artifact ---\n")
    );
}

fn assert_failure_focus_line(output: &str, command: &str, artifact_tool: &str) {
    assert_failure_focus_line_with_artifact_count(output, command, artifact_tool, 1);
}

fn assert_failure_focus_line_with_artifact_count(
    output: &str,
    command: &str,
    artifact_tool: &str,
    artifact_count: usize,
) {
    let line = output
        .lines()
        .find(|line| line.starts_with(&format!("{command} | ")))
        .unwrap_or_else(|| panic!("missing failure line for {command} in\n{output}"));
    assert!(
        line.contains(&format!(
            "| 1 | 0 | 1 | 0 | 0 | {artifact_count} | {artifact_tool} |"
        )),
        "{command} should show one failure with unknown detail coverage and {artifact_count} retained artifact(s) in\n{line}"
    );
    assert!(
        line.contains(&format!("| ~/.cx/cache/failures/{artifact_tool}/")),
        "{command} should point at {artifact_count} {artifact_tool} artifact(s) in\n{line}"
    );
}

fn expected_artifact_counts(cases: &[FailureCase]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for case in cases {
        *counts.entry(case.artifact_tool).or_insert(0) += 1;
    }
    counts
}

fn assert_insights_commands_are_not_recorded(home: &Path) {
    let connection = Connection::open(home.join(".cx/db.sqlite")).unwrap();
    let count: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM command_invocations WHERE command = 'insights'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

fn assert_recorded_expansion_reason(home: &Path, command: &str, expected: &str) {
    let connection = Connection::open(home.join(".cx/db.sqlite")).unwrap();
    let reason: String = connection
        .query_row(
            "
                SELECT expansion_reason
                FROM command_invocations
                WHERE command_family = ?1
                ORDER BY id DESC
                LIMIT 1
                ",
            [command],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reason, expected);
}

fn assert_expansion_reason(case: &FailureCase, expected: &str) {
    assert_eq!(
        case.outcome
            .observation
            .as_ref()
            .and_then(|observation| observation.expansion_reason.as_deref()),
        Some(expected),
        "{case:?}"
    );
}

fn expected_expansion_reason(command: &str) -> &'static str {
    match command {
        "grep" => "no-match-summary",
        "node check" => "syntax-check-summary",
        _ => "failure-artifact-recovery-hint",
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

struct ProjectTempDir {
    path: PathBuf,
}

impl ProjectTempDir {
    fn new(label: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".tmp");
        let unique = format!("cx-test-{label}-{}-{}", std::process::id(), now_ns());
        let path = root.join(unique);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProjectTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    original_path: Option<OsString>,
    original_disable_fallback: Option<OsString>,
    original_extra_fallback: Option<OsString>,
    original_home: Option<OsString>,
    original_disable_insights: Option<OsString>,
    original_enable_insights: Option<OsString>,
    original_enable_passthrough: Option<OsString>,
}

impl EnvGuard {
    fn fake_path(bin: &Path, home: &Path) -> Self {
        let lock = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let guard = Self {
            _lock: lock,
            original_path: std::env::var_os("PATH"),
            original_disable_fallback: std::env::var_os("CX_DISABLE_TOOL_FALLBACK_PATHS"),
            original_extra_fallback: std::env::var_os("CX_TOOL_FALLBACK_PATHS"),
            original_home: std::env::var_os("HOME"),
            original_disable_insights: std::env::var_os("CX_DISABLE_INSIGHTS"),
            original_enable_insights: std::env::var_os("CX_ENABLE_INSIGHTS"),
            original_enable_passthrough: std::env::var_os("CX_ENABLE_UNSUPPORTED_PASSTHROUGH"),
        };
        std::env::set_var("PATH", bin);
        std::env::set_var("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
        std::env::remove_var("CX_TOOL_FALLBACK_PATHS");
        std::env::set_var("HOME", home);
        std::env::remove_var("CX_DISABLE_INSIGHTS");
        std::env::set_var("CX_ENABLE_INSIGHTS", "1");
        std::env::set_var("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1");
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        restore_env("PATH", self.original_path.take());
        restore_env(
            "CX_DISABLE_TOOL_FALLBACK_PATHS",
            self.original_disable_fallback.take(),
        );
        restore_env(
            "CX_TOOL_FALLBACK_PATHS",
            self.original_extra_fallback.take(),
        );
        restore_env("HOME", self.original_home.take());
        restore_env("CX_DISABLE_INSIGHTS", self.original_disable_insights.take());
        restore_env("CX_ENABLE_INSIGHTS", self.original_enable_insights.take());
        restore_env(
            "CX_ENABLE_UNSUPPORTED_PASSTHROUGH",
            self.original_enable_passthrough.take(),
        );
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn write_executable(dir: &Path, name: &str, body: &str) {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn restore_env(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
