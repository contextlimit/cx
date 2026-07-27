use std::hint::black_box;

use criterion::Criterion;
use cx::commands::{cmake_cmd, tsc_cmd};

use crate::hot_paths::config::configure_process_group;
use crate::support;

struct BuildWrapperBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    cmake_failure_args: Vec<String>,
    cmake_success_args: Vec<String>,
    tsc_args: Vec<String>,
}

fn setup_build_wrapper_bench() -> BuildWrapperBenchSetup {
    let temp = support::ProjectTempDir::new("build-wrappers");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let cmake_raw = support::cmake_failure_fixture(140);
    let cmake_success_raw = support::cmake_success_fixture(180);
    let tsc_raw = support::tsc_failure_fixture(120);
    let cmake_out = temp.path().join("cmake.out");
    let cmake_success_out = temp.path().join("cmake-success.out");
    let tsc_out = temp.path().join("tsc.out");
    support::write_file(&cmake_out, &cmake_raw);
    support::write_file(&cmake_success_out, &cmake_success_raw);
    support::write_file(&tsc_out, &tsc_raw);
    support::write_executable(
        &bin,
        "cmake",
        &format!(
            "#!/bin/sh\ncase \" $* \" in\n  *\" cx \"*) /bin/cat {}; exit 0 ;;\n  *) /bin/cat {}; exit 2 ;;\nesac\n",
            support::shell_quote(&cmake_success_out),
            support::shell_quote(&cmake_out),
        ),
    );
    support::write_executable(
        &bin,
        "tsc",
        &support::output_script(Some(&tsc_out), None, 2),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let cmake_failure_args = vec![
        "build".to_string(),
        "--target".to_string(),
        "cx-failure".to_string(),
        "-j".to_string(),
        "8".to_string(),
    ];
    let cmake_once = cmake_cmd::run_build(&cmake_failure_args).unwrap();
    support::assert_reduction(
        "cmake wrapper",
        &cmake_raw,
        &cmake_once.stdout,
        0.45,
        &[
            "cmake build: failed",
            "missing_runtime.cpp",
            "ninja: build stopped",
        ],
    );

    let cmake_success_args = vec![
        "build".to_string(),
        "--target".to_string(),
        "cx".to_string(),
        "-j".to_string(),
        "8".to_string(),
    ];
    let cmake_success_once = cmake_cmd::run_build(&cmake_success_args).unwrap();
    support::assert_reduction(
        "cmake success wrapper",
        &cmake_success_raw,
        &cmake_success_once.stdout,
        0.45,
        &["cmake build: ok", "Built target cx", "lines omitted"],
    );

    let tsc_args = vec![
        "--noEmit".to_string(),
        "--project".to_string(),
        "tsconfig.json".to_string(),
    ];
    let tsc_once = tsc_cmd::run(&tsc_args).unwrap();
    support::assert_reduction(
        "tsc wrapper",
        &tsc_raw,
        &tsc_once.stdout,
        0.45,
        &["TypeScript: 3 errors", "TS2322", "src/view.tsx"],
    );

    BuildWrapperBenchSetup {
        _env,
        _temp: temp,
        cmake_failure_args,
        cmake_success_args,
        tsc_args,
    }
}

pub fn bench_build_wrappers(c: &mut Criterion) {
    let setup = setup_build_wrapper_bench();
    let mut group = c.benchmark_group("build_wrappers");
    configure_process_group(&mut group);
    group.bench_function("cmake_failure", |b| {
        b.iter(|| cmake_cmd::run_build(black_box(&setup.cmake_failure_args)))
    });
    group.bench_function("cmake_success", |b| {
        b.iter(|| cmake_cmd::run_build(black_box(&setup.cmake_success_args)))
    });
    group.bench_function("tsc_failure", |b| {
        b.iter(|| tsc_cmd::run(black_box(&setup.tsc_args)))
    });
    group.finish();
}
