#![cfg(unix)]

use std::fs;

use cx::commands::cmake_cmd;

#[allow(dead_code)]
#[path = "../benches/support/mod.rs"]
mod support;

fn warning_flood_failure_fixture() -> String {
    let mut output = String::new();
    for _ in 0..96 {
        output.push_str("ld: warning: ignoring duplicate libraries: '-lc++'\n");
    }
    output.push_str(
        "src/runtime.cpp:42:9: warning: ignoring return value of function declared with 'nodiscard' attribute\n",
    );
    output.push_str(
        "src/config.cpp:17:5: warning: field designator 'name' does not match declaration order\n",
    );
    output.push_str("The command could not be loaded, possibly because:\n");
    output.push_str("  * You intended to execute a .NET application:\n");
    output.push_str("      The application 'build' does not exist.\n");
    output.push_str("  * You intended to execute a .NET SDK command:\n");
    output.push_str("      A compatible .NET SDK was not found.\n");
    output.push_str("Requested SDK version: 8.0.125\n");
    output.push_str("global.json file: /workspace/global.json\n");
    output.push_str("Installed SDKs:\n");
    output.push_str("8.0.128 [/usr/local/share/dotnet/sdk]\n");
    output.push_str("make[2]: *** [generated/service.stamp] Error 145\n");
    output.push_str("make[1]: *** [all] Error 2\n");
    output.push_str("make: *** [all] Error 2\n");
    output
}

fn run_failure(raw: &str) -> cx::support::runner::ProxyOutcome {
    let temp = support::ProjectTempDir::new("cmake-failure-summary");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let output_path = temp.path().join("cmake.out");
    fs::create_dir_all(&home).unwrap();
    support::write_file(&output_path, raw);
    support::write_executable(
        &bin,
        "cmake",
        &support::output_script(Some(&output_path), None, 2),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    cmake_cmd::run_build(&[
        "build".to_string(),
        "--target".to_string(),
        "service".to_string(),
    ])
    .unwrap()
}

#[test]
fn warning_flood_keeps_terminal_sdk_and_make_failure_evidence() {
    let raw = warning_flood_failure_fixture();
    let outcome = run_failure(&raw);

    assert_eq!(outcome.exit_code, 2);
    assert!(outcome.stdout.contains("cmake build: failed"));
    assert!(outcome
        .stdout
        .contains("A compatible .NET SDK was not found."));
    assert!(outcome.stdout.contains("Requested SDK version: 8.0.125"));
    assert!(outcome.stdout.contains("Error 145"));
    assert!(outcome.stdout.contains("make: *** [all] Error 2"));
    assert!(outcome.stdout.contains("nodiscard"));
    assert!(outcome.stdout.contains("field designator"));
    assert!(outcome.stdout.contains("repeated warning lines suppressed"));
    assert_eq!(
        outcome
            .stdout
            .matches("ld: warning: ignoring duplicate libraries: '-lc++'")
            .count(),
        1,
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.len() * 100 <= raw.len() * 45,
        "expected at least 55% reduction\nraw bytes: {}\nemitted bytes: {}\n{}",
        raw.len(),
        outcome.stdout.len(),
        outcome.stdout
    );
}

#[test]
fn early_compiler_error_and_late_ninja_stop_remain_in_native_order() {
    let mut raw = String::from(
        "FAILED: src/CMakeFiles/app.dir/runtime.cpp.o\nsrc/runtime.cpp:11:7: error: unknown type name 'RuntimeState'\n",
    );
    for _ in 0..90 {
        raw.push_str("ld: warning: ignoring duplicate libraries: '-lc++'\n");
    }
    raw.push_str("ninja: build stopped: subcommand failed.\n");

    let outcome = run_failure(&raw);
    let compiler = outcome.stdout.find("unknown type name").unwrap();
    let terminal = outcome.stdout.find("ninja: build stopped").unwrap();

    assert!(compiler < terminal, "{}", outcome.stdout);
    assert!(outcome.stdout.contains("repeated warning lines suppressed"));
}

#[test]
fn only_exact_repeated_warnings_are_deduplicated() {
    let raw = warning_flood_failure_fixture();
    let outcome = run_failure(&raw);

    assert!(outcome.stdout.contains("src/runtime.cpp:42:9: warning:"));
    assert!(outcome.stdout.contains("src/config.cpp:17:5: warning:"));
}
