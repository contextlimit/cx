use super::*;

#[test]
fn restore_double_dash_reinserts_separator() {
    let args = vec!["crate_name".to_string(), "--nocapture".to_string()];
    let raw = vec![
        "cx".to_string(),
        "cargo".to_string(),
        "test".to_string(),
        "crate_name".to_string(),
        "--".to_string(),
        "--nocapture".to_string(),
    ];
    assert_eq!(
        restore_double_dash_with_raw(&args, &raw),
        vec![
            "crate_name".to_string(),
            "--".to_string(),
            "--nocapture".to_string()
        ]
    );
}

#[test]
fn restore_double_dash_ignores_explicit_cx_separator() {
    let args = vec!["--lib".to_string(), "crate_name".to_string()];
    let raw = vec![
        "cx".to_string(),
        "--".to_string(),
        "cargo".to_string(),
        "test".to_string(),
        "--lib".to_string(),
        "crate_name".to_string(),
    ];
    assert_eq!(restore_double_dash_with_raw(&args, &raw), args);
}

#[test]
fn restore_double_dash_handles_explicit_cx_and_harness_separators() {
    let args = vec!["crate_name".to_string(), "--nocapture".to_string()];
    let raw = vec![
        "cx".to_string(),
        "--".to_string(),
        "cargo".to_string(),
        "test".to_string(),
        "crate_name".to_string(),
        "--".to_string(),
        "--nocapture".to_string(),
    ];
    assert_eq!(
        restore_double_dash_with_raw(&args, &raw),
        vec![
            "crate_name".to_string(),
            "--".to_string(),
            "--nocapture".to_string()
        ]
    );
}

#[test]
fn split_filter_plan_accepts_multiple_filters_with_exact() {
    let plan = SplitFilterTestPlan::from_args(&[
        "alpha".to_string(),
        "beta".to_string(),
        "--".to_string(),
        "--exact".to_string(),
    ])
    .unwrap();
    assert!(plan.prefix_args.is_empty());
    assert_eq!(plan.filters, ["alpha", "beta"]);
    assert_eq!(plan.harness_args, ["--exact"]);
    assert!(plan.had_separator);
    assert_eq!(
        plan.args_for_filter("alpha"),
        ["alpha", "--", "--exact"].map(String::from)
    );
}

#[test]
fn split_filter_plan_preserves_package_prefix() {
    let plan = SplitFilterTestPlan::from_args(&[
        "-p".to_string(),
        "clob-engine".to_string(),
        "committed_fee_payout_batch_reconciles_open_obligations_with_manifest".to_string(),
        "committed_replay_records_fee_obligation_reconciliation".to_string(),
    ])
    .unwrap();
    assert_eq!(plan.prefix_args, ["-p", "clob-engine"]);
    assert_eq!(
        plan.filters,
        [
            "committed_fee_payout_batch_reconciles_open_obligations_with_manifest",
            "committed_replay_records_fee_obligation_reconciliation"
        ]
    );
    assert!(!plan.had_separator);
    assert_eq!(
        plan.args_for_filter("committed_replay_records_fee_obligation_reconciliation"),
        [
            "-p",
            "clob-engine",
            "committed_replay_records_fee_obligation_reconciliation"
        ]
        .map(String::from)
    );
}

#[test]
fn split_filter_plan_preserves_target_flags_and_harness_args() {
    let plan = SplitFilterTestPlan::from_args(&[
        "--workspace".to_string(),
        "--test".to_string(),
        "fee_tests".to_string(),
        "alpha".to_string(),
        "beta".to_string(),
        "--".to_string(),
        "--nocapture".to_string(),
    ])
    .unwrap();
    assert_eq!(plan.prefix_args, ["--workspace", "--test", "fee_tests"]);
    assert_eq!(plan.filters, ["alpha", "beta"]);
    assert_eq!(plan.harness_args, ["--nocapture"]);
    assert_eq!(
        plan.args_for_filter("beta"),
        [
            "--workspace",
            "--test",
            "fee_tests",
            "beta",
            "--",
            "--nocapture"
        ]
        .map(String::from)
    );
}

#[test]
fn split_filter_plan_rejects_single_filter_or_options_after_filters() {
    assert!(SplitFilterTestPlan::from_args(&[
        "--package".to_string(),
        "cx".to_string(),
        "alpha".to_string(),
    ])
    .is_none());
    assert!(SplitFilterTestPlan::from_args(&[
        "alpha".to_string(),
        "--workspace".to_string(),
        "beta".to_string(),
    ])
    .is_none());
}

#[cfg(unix)]
#[test]
fn run_test_expands_multiple_filters_with_package_prefix() {
    crate::support::test_support::with_fake_path(
            &[(
                "cargo",
                "#!/bin/sh\ncase \"$*\" in\n  \"test -p clob-engine alpha\"|\"test -p clob-engine beta\") printf 'running 1 test\\n\\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\\n' ;;\n  *) printf 'unexpected cargo args: %s\\n' \"$*\" >&2; exit 9 ;;\nesac\n",
            )],
            || {
                let output = run_test(&[
                    "-p".to_string(),
                    "clob-engine".to_string(),
                    "alpha".to_string(),
                    "beta".to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(
                    output
                        .stdout
                        .contains("cargo test: split 2 filters into 2 cargo test runs")
                );
                assert!(output.stdout.contains("[1/2] alpha (exit 0)"));
                assert!(output.stdout.contains("[2/2] beta (exit 0)"));
                assert_eq!(
                    output.observation.as_ref().unwrap().source,
                    "cargo test split-filters"
                );
            },
        );
}

#[test]
fn filter_cargo_test_passes_keep_summary() {
    let output = "running 1 test\ntest sample ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s";
    let filtered = filter_cargo_test(output);
    assert!(filtered.contains("test result: ok."));
}

#[test]
fn filter_cargo_test_failures_are_compacted() {
    let output = "running 1 test\n\nfailures:\n\n---- sample stdout ----\nthread 'sample' panicked at src/lib.rs:10:5\n\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s";
    let filtered = filter_cargo_test(output);
    assert!(filtered.contains("FAILURES (1):"));
    assert!(filtered.contains("thread 'sample' panicked"));
}

#[test]
fn filter_cargo_compile_failure_formats_primary_diagnostic() {
    let output = "Compiling demo v0.1.0 (/tmp/demo)\nerror[E0425]: cannot find value `missing` in this scope\n --> src/lib.rs:2:5\n  |\n2 |     missing\n  |     ^^^^^^^ not found in this scope\n\nerror: could not compile `demo` (lib test) due to 1 previous error";
    let filtered = filter_cargo_test(output);
    assert!(filtered.contains("cargo test: compile failed"));
    assert!(filtered.contains("[E0425]"));
    assert!(filtered.contains("src/lib.rs:2:5"));
    assert!(filtered.contains("could not compile"));
}

#[test]
fn filter_cargo_compile_failure_keeps_warning_lines() {
    let output = "warning: unused import: `std::fmt`\nerror: proc macro panicked\n --> src/main.rs:4:1\n  |\n4 | boom!();\n  | ^^^^^^^\n\nerror: could not compile `demo` (bin \"demo\" test) due to 1 previous error; 1 warning emitted";
    let filtered = filter_cargo_test(output);
    assert!(filtered.contains("proc macro panicked"));
    assert!(filtered.contains("warning: unused import"));
    assert!(filtered.contains("could not compile"));
}

#[cfg(unix)]
#[test]
fn run_test_uses_fake_cargo_binary() {
    crate::support::test_support::with_fake_path(
            &[(
                "cargo",
                "#!/bin/sh\ncat <<'EOF'\nrunning 1 test\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\nEOF\n",
            )],
            || {
                let output = run_test(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("test result: ok."));
            },
        );
}
