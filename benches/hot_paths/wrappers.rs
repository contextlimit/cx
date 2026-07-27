use std::hint::black_box;

use criterion::Criterion;
use cx::commands::{cargo_cmd, ctest_cmd, go_cmd, pytest_cmd};

use crate::hot_paths::config::configure_process_group;
use crate::support;

struct WrapperBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    empty_args: Vec<String>,
    ctest_list_args: Vec<String>,
}

fn setup_wrapper_bench() -> WrapperBenchSetup {
    let temp = support::ProjectTempDir::new("wrappers");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let cargo_raw = support::cargo_compile_failure_fixture(140);
    let pytest_raw = support::pytest_failure_fixture(120);
    let go_raw = support::go_json_failure_fixture(180);
    let ctest_raw = support::ctest_failure_fixture(160);
    let ctest_list_raw = support::ctest_list_fixture(180);
    let cargo_out = temp.path().join("cargo.out");
    let pytest_out = temp.path().join("pytest.out");
    let go_out = temp.path().join("go.out");
    let ctest_out = temp.path().join("ctest.out");
    let ctest_list_out = temp.path().join("ctest-list.out");
    support::write_file(&cargo_out, &cargo_raw);
    support::write_file(&pytest_out, &pytest_raw);
    support::write_file(&go_out, &go_raw);
    support::write_file(&ctest_out, &ctest_raw);
    support::write_file(&ctest_list_out, &ctest_list_raw);
    write_wrapper_binaries(
        &bin,
        &cargo_out,
        &pytest_out,
        &go_out,
        &ctest_out,
        &ctest_list_out,
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    assert_wrapper_reductions(&cargo_raw, &pytest_raw, &go_raw, &ctest_raw);
    let ctest_list_args = vec![
        "--test-dir".to_string(),
        "build-web".to_string(),
        "-N".to_string(),
        "-R".to_string(),
        "sample-ui-catalog-model-test".to_string(),
    ];
    let ctest_list_once = ctest_cmd::run(&ctest_list_args).unwrap();
    support::assert_reduction(
        "ctest list wrapper",
        &ctest_list_raw,
        &ctest_list_once.stdout,
        0.85,
        &["ctest: list", "sample-ui-catalog-model-test-179"],
    );

    WrapperBenchSetup {
        _env,
        _temp: temp,
        empty_args: Vec::new(),
        ctest_list_args,
    }
}

fn write_wrapper_binaries(
    bin: &std::path::Path,
    cargo_out: &std::path::Path,
    pytest_out: &std::path::Path,
    go_out: &std::path::Path,
    ctest_out: &std::path::Path,
    ctest_list_out: &std::path::Path,
) {
    support::write_executable(
        bin,
        "cargo",
        &support::output_script(Some(cargo_out), None, 101),
    );
    support::write_executable(
        bin,
        "pytest",
        &support::output_script(Some(pytest_out), None, 1),
    );
    support::write_executable(bin, "go", &support::output_script(Some(go_out), None, 1));
    support::write_executable(
        bin,
        "ctest",
        &format!(
            "#!/bin/sh\ncase \" $* \" in\n  *\" -N \"*) /bin/cat {}; exit 0 ;;\n  *) /bin/cat {}; exit 8 ;;\nesac\n",
            support::shell_quote(ctest_list_out),
            support::shell_quote(ctest_out)
        ),
    );
}

fn assert_wrapper_reductions(cargo_raw: &str, pytest_raw: &str, go_raw: &str, ctest_raw: &str) {
    let cargo_once = cargo_cmd::run_test(&[]).unwrap();
    support::assert_reduction(
        "cargo wrapper",
        cargo_raw,
        &cargo_once.stdout,
        0.30,
        &["E0425", "src/lib.rs:42:9", "could not compile"],
    );
    let pytest_once = pytest_cmd::run(&[]).unwrap();
    support::assert_reduction(
        "pytest wrapper",
        pytest_raw,
        &pytest_once.stdout,
        0.20,
        &["Pytest: 119 passed, 1 failed", "test_alpha"],
    );
    let go_once = go_cmd::run_test(&[]).unwrap();
    support::assert_reduction(
        "go wrapper",
        go_raw,
        &go_once.stdout,
        0.20,
        &["Go test:", "TestWidget", "expected 5, got 3"],
    );
    let ctest_once = ctest_cmd::run(&[]).unwrap();
    support::assert_reduction(
        "ctest wrapper",
        ctest_raw,
        &ctest_once.stdout,
        0.20,
        &["ctest: failed", "sample-ui-dockview-perf-mouse-e2e"],
    );
}

pub fn bench_wrappers(c: &mut Criterion) {
    let setup = setup_wrapper_bench();
    let mut group = c.benchmark_group("wrappers");
    configure_process_group(&mut group);
    group.bench_function("cargo_compile_failure", |b| {
        b.iter(|| cargo_cmd::run_test(black_box(&setup.empty_args)))
    });
    group.bench_function("pytest_failure", |b| {
        b.iter(|| pytest_cmd::run(black_box(&setup.empty_args)))
    });
    group.bench_function("go_json_failure", |b| {
        b.iter(|| go_cmd::run_test(black_box(&setup.empty_args)))
    });
    group.bench_function("ctest_failure", |b| {
        b.iter(|| ctest_cmd::run(black_box(&setup.empty_args)))
    });
    group.bench_function("ctest_list", |b| {
        b.iter(|| ctest_cmd::run(black_box(&setup.ctest_list_args)))
    });
    group.finish();
}
