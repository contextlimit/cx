use std::hint::black_box;
use std::process::Command;
use std::time::Duration;

use criterion::Criterion;
use cx::support::runner::{capture, capture_with_stdin_timeout, run_filtered, RunOptions};

use crate::hot_paths::config::configure_runner_group;
use crate::hot_paths::util::numbered_lines;
use crate::support;

struct RunnerBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    stdin_payload: Vec<u8>,
}

fn setup_runner_bench() -> RunnerBenchSetup {
    let temp = support::ProjectTempDir::new("runner");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let small_stdout = temp.path().join("small.txt");
    let large_stdout = temp.path().join("large.txt");
    let large_stderr = temp.path().join("large.err");
    support::write_file(&small_stdout, "done\n");
    support::write_file(&large_stdout, &numbered_lines("stdout line", 700));
    support::write_file(&large_stderr, &numbered_lines("stderr line", 700));
    support::write_executable(
        &bin,
        "small-output",
        &support::output_script(Some(&small_stdout), None, 0),
    );
    support::write_executable(
        &bin,
        "large-output",
        &support::output_script(Some(&large_stdout), None, 0),
    );
    support::write_executable(
        &bin,
        "large-stderr",
        &support::output_script(None, Some(&large_stderr), 2),
    );
    support::write_executable(
        &bin,
        "descriptor-holder",
        "#!/bin/sh\n(/bin/sleep 1) &\nprintf 'done\\n'\nexit 0\n",
    );
    support::write_executable(
        &bin,
        "stdin-capture",
        "#!/bin/sh\ncat >/dev/null\nprintf 'done\\n'\n",
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let fallback = run_filtered(
        Command::new("large-output"),
        "large-output",
        |_| None::<String>,
        RunOptions::default().fallback_window(8, 8),
    )
    .unwrap();
    support::assert_reduction(
        "runner fallback",
        &numbered_lines("stdout line", 700),
        &fallback.stdout,
        0.08,
        &["stdout line 0000", "stdout line 0699"],
    );
    let stdin_payload = vec![b'x'; 16 * 1024];
    let stdin_once = capture_with_stdin_timeout(
        Command::new("stdin-capture"),
        "stdin-capture",
        stdin_payload.clone(),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(stdin_once.stdout.trim(), "done");

    RunnerBenchSetup {
        _env,
        _temp: temp,
        stdin_payload,
    }
}

pub fn bench_runner(c: &mut Criterion) {
    let setup = setup_runner_bench();
    let mut group = c.benchmark_group("runner");
    configure_runner_group(&mut group);
    group.bench_function("capture_small_stdout", |b| {
        b.iter(|| {
            capture(
                black_box(Command::new("small-output")),
                black_box("small-output"),
            )
        })
    });
    group.bench_function("capture_large_stdout", |b| {
        b.iter(|| {
            capture(
                black_box(Command::new("large-output")),
                black_box("large-output"),
            )
        })
    });
    group.bench_function("capture_large_stderr_failure", |b| {
        b.iter(|| {
            capture(
                black_box(Command::new("large-stderr")),
                black_box("large-stderr"),
            )
        })
    });
    group.bench_function("capture_descriptor_inheritance", |b| {
        b.iter(|| {
            capture(
                black_box(Command::new("descriptor-holder")),
                black_box("descriptor-holder"),
            )
        })
    });
    group.bench_function("capture_with_stdin_timeout", |b| {
        b.iter(|| {
            capture_with_stdin_timeout(
                black_box(Command::new("stdin-capture")),
                black_box("stdin-capture"),
                black_box(setup.stdin_payload.clone()),
                black_box(Duration::from_secs(1)),
            )
        })
    });
    group.bench_function("filtered_fallback_window", |b| {
        b.iter(|| {
            run_filtered(
                black_box(Command::new("large-output")),
                black_box("large-output"),
                |_| None::<String>,
                black_box(RunOptions::default().fallback_window(8, 8)),
            )
        })
    });
    group.finish();
}
