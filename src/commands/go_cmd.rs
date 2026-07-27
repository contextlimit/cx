use std::collections::BTreeMap;

use anyhow::Result;
use serde::Deserialize;

use crate::support::runner::{run_filtered, ProxyOutcome, RunOptions};
use crate::support::utils::{resolved_command, truncate};

#[derive(Debug, Deserialize)]
struct GoTestEvent {
    #[serde(rename = "Action")]
    action: String,
    #[serde(rename = "Package")]
    package: Option<String>,
    #[serde(rename = "Test")]
    test: Option<String>,
    #[serde(rename = "Output")]
    output: Option<String>,
    #[serde(rename = "ImportPath")]
    import_path: Option<String>,
    #[serde(rename = "FailedBuild")]
    failed_build: Option<String>,
}

#[derive(Debug, Default)]
struct PackageResult {
    pass: usize,
    fail: usize,
    skip: usize,
    build_failed: bool,
    build_errors: Vec<String>,
    failed_tests: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Default)]
struct GoTestState {
    packages: BTreeMap<String, PackageResult>,
    current_test_output: BTreeMap<(String, String), Vec<String>>,
    build_output: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GoTotals {
    packages: usize,
    pass: usize,
    fail: usize,
    skip: usize,
    build_fail: usize,
}

pub fn run_test(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("go");
    cmd.arg("test");
    if !args.iter().any(|arg| arg == "-json") {
        cmd.arg("-json");
    }
    for arg in args {
        cmd.arg(arg);
    }

    run_filtered(
        cmd,
        "go",
        |output| Some(filter_go_test_json(&output.stdout)),
        RunOptions::stdout_only(),
    )
}

fn filter_go_test_json(output: &str) -> String {
    let state = collect_go_test_state(output);
    render_go_test_summary(&state.packages)
}

fn collect_go_test_state(output: &str) -> GoTestState {
    let mut state = GoTestState::default();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<GoTestEvent>(trimmed) else {
            continue;
        };
        apply_go_event(&mut state, event);
    }
    state
}

fn apply_go_event(state: &mut GoTestState, event: GoTestEvent) {
    match event.action.as_str() {
        "build-output" => record_build_output(state, &event),
        "build-fail" => {}
        _ => record_package_event(state, event),
    }
}

fn record_build_output(state: &mut GoTestState, event: &GoTestEvent) {
    if let (Some(import_path), Some(output_text)) = (&event.import_path, &event.output) {
        let text = output_text.trim_end().to_string();
        if !text.is_empty() {
            state
                .build_output
                .entry(import_path.clone())
                .or_default()
                .push(text);
        }
    }
}

fn record_package_event(state: &mut GoTestState, event: GoTestEvent) {
    let package = event.package.unwrap_or_else(|| "unknown".to_string());
    let package_result = state.packages.entry(package.clone()).or_default();

    match event.action.as_str() {
        "pass" if event.test.is_some() => package_result.pass += 1,
        "fail" => {
            if let Some(test) = &event.test {
                package_result.fail += 1;
                let key = (package.clone(), test.clone());
                let outputs = state.current_test_output.remove(&key).unwrap_or_default();
                package_result.failed_tests.push((test.clone(), outputs));
            } else if event.failed_build.is_some() {
                package_result.build_failed = true;
                if let Some(import_path) = &event.failed_build {
                    if let Some(errors) = state.build_output.remove(import_path) {
                        package_result.build_errors = errors;
                    }
                }
            }
        }
        "skip" if event.test.is_some() => package_result.skip += 1,
        "output" => {
            if let (Some(test), Some(output_text)) = (&event.test, &event.output) {
                state
                    .current_test_output
                    .entry((package.clone(), test.clone()))
                    .or_default()
                    .push(output_text.trim_end().to_string());
            }
        }
        _ => {}
    }
}

fn render_go_test_summary(packages: &BTreeMap<String, PackageResult>) -> String {
    let totals = go_totals(packages);

    if totals.fail == 0 && totals.build_fail == 0 && totals.pass == 0 {
        return "Go test: No tests found".to_string();
    }
    if totals.fail == 0 && totals.build_fail == 0 {
        return format!(
            "Go test: {} passed in {} packages",
            totals.pass, totals.packages
        );
    }

    render_go_failures(packages, totals)
}

fn go_totals(packages: &BTreeMap<String, PackageResult>) -> GoTotals {
    GoTotals {
        packages: packages.len(),
        pass: packages.values().map(|package| package.pass).sum(),
        fail: packages.values().map(|package| package.fail).sum(),
        skip: packages.values().map(|package| package.skip).sum(),
        build_fail: packages
            .values()
            .filter(|package| package.build_failed)
            .count(),
    }
}

fn render_go_failures(packages: &BTreeMap<String, PackageResult>, totals: GoTotals) -> String {
    let mut result = String::new();
    result.push_str(&format!(
        "Go test: {} passed, {} failed",
        totals.pass,
        totals.fail + totals.build_fail
    ));
    if totals.skip > 0 {
        result.push_str(&format!(", {} skipped", totals.skip));
    }
    result.push_str(&format!(" in {} packages\n", totals.packages));
    result.push_str("═══════════════════════════════════════\n");

    render_go_build_failures(&mut result, packages);
    render_go_test_failures(&mut result, packages);

    result.trim().to_string()
}

fn render_go_build_failures(result: &mut String, packages: &BTreeMap<String, PackageResult>) {
    for (package, package_result) in packages
        .iter()
        .filter(|(_, package_result)| package_result.build_failed)
    {
        result.push_str(&format!(
            "\n{} [build failed]\n",
            compact_package_name(package)
        ));
        for line in &package_result.build_errors {
            let trimmed = line.trim();
            if !trimmed.starts_with('#') && !trimmed.is_empty() {
                result.push_str(&format!("  {}\n", truncate(trimmed, 120)));
            }
        }
    }
}

fn render_go_test_failures(result: &mut String, packages: &BTreeMap<String, PackageResult>) {
    for (package, package_result) in packages.iter() {
        if package_result.fail == 0 {
            continue;
        }
        result.push_str(&format!(
            "\n{} ({} passed, {} failed)\n",
            compact_package_name(package),
            package_result.pass,
            package_result.fail
        ));
        for (test, outputs) in &package_result.failed_tests {
            result.push_str(&format!("  [FAIL] {test}\n"));
            for line in outputs
                .iter()
                .filter(|line| {
                    let lower = line.to_ascii_lowercase();
                    !line.trim().is_empty()
                        && !line.starts_with("=== RUN")
                        && !line.starts_with("--- FAIL")
                        && (lower.contains("error")
                            || lower.contains("expected")
                            || lower.contains("got")
                            || lower.contains("panic")
                            || line.trim().starts_with("at "))
                })
                .take(5)
            {
                result.push_str(&format!("     {}\n", truncate(line, 100)));
            }
        }
    }
}

fn compact_package_name(package: &str) -> String {
    package.rsplit('/').next().unwrap_or(package).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_go_test_json_all_pass() {
        let output = "{\"Action\":\"run\",\"Package\":\"example.com/foo\",\"Test\":\"TestBar\"}\n\
                      {\"Action\":\"pass\",\"Package\":\"example.com/foo\",\"Test\":\"TestBar\"}\n\
                      {\"Action\":\"pass\",\"Package\":\"example.com/foo\"}";
        let filtered = filter_go_test_json(output);
        assert!(filtered.contains("1 passed"));
    }

    #[test]
    fn filter_go_test_json_counts_only_test_level_skip_events() {
        let output =
            "{\"Action\":\"skip\",\"Package\":\"example.com/foo\",\"Test\":\"TestSkipped\"}\n\
                      {\"Action\":\"skip\",\"Package\":\"example.com/foo\"}\n\
                      {\"Action\":\"pass\",\"Package\":\"example.com/foo\",\"Test\":\"TestBar\"}\n\
                      {\"Action\":\"pass\",\"Package\":\"example.com/foo\"}";
        let state = collect_go_test_state(output);
        let package = state.packages.get("example.com/foo").unwrap();
        assert_eq!(package.pass, 1);
        assert_eq!(package.skip, 1);
    }

    #[test]
    fn filter_go_test_json_with_failure() {
        let output = "{\"Action\":\"run\",\"Package\":\"example.com/foo\",\"Test\":\"TestFail\"}\n\
                      {\"Action\":\"output\",\"Package\":\"example.com/foo\",\"Test\":\"TestFail\",\"Output\":\"    Error: expected 5, got 3\\n\"}\n\
                      {\"Action\":\"fail\",\"Package\":\"example.com/foo\",\"Test\":\"TestFail\"}\n\
                      {\"Action\":\"fail\",\"Package\":\"example.com/foo\"}";
        let filtered = filter_go_test_json(output);
        assert!(filtered.contains("1 failed"));
        assert!(filtered.contains("TestFail"));
    }

    #[test]
    fn filter_go_test_json_orders_failed_packages_deterministically() {
        let output = "{\"Action\":\"output\",\"Package\":\"example.com/zeta\",\"Test\":\"TestZ\",\"Output\":\"    Error: z failed\\n\"}\n\
                      {\"Action\":\"fail\",\"Package\":\"example.com/zeta\",\"Test\":\"TestZ\"}\n\
                      {\"Action\":\"fail\",\"Package\":\"example.com/zeta\"}\n\
                      {\"Action\":\"output\",\"Package\":\"example.com/alpha\",\"Test\":\"TestA\",\"Output\":\"    Error: a failed\\n\"}\n\
                      {\"Action\":\"fail\",\"Package\":\"example.com/alpha\",\"Test\":\"TestA\"}\n\
                      {\"Action\":\"fail\",\"Package\":\"example.com/alpha\"}";
        let filtered = filter_go_test_json(output);
        let alpha = filtered.find("alpha (0 passed, 1 failed)").unwrap();
        let zeta = filtered.find("zeta (0 passed, 1 failed)").unwrap();
        assert!(alpha < zeta, "{filtered}");
    }

    #[cfg(unix)]
    #[test]
    fn run_test_uses_fake_go_binary() {
        crate::support::test_support::with_fake_path(
            &[(
                "go",
                "#!/bin/sh\ncat <<'EOF'\n{\"Action\":\"run\",\"Package\":\"example.com/foo\",\"Test\":\"TestBar\"}\n{\"Action\":\"pass\",\"Package\":\"example.com/foo\",\"Test\":\"TestBar\"}\n{\"Action\":\"pass\",\"Package\":\"example.com/foo\"}\nEOF\n",
            )],
            || {
                let output = run_test(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("1 passed"));
            },
        );
    }
}
