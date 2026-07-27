use std::hint::black_box;

use criterion::Criterion;
use cx::commands::node_cmd;

use crate::hot_paths::config::configure_process_group;
use crate::hot_paths::util::strings;
use crate::support;

struct NodeCheckBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    js_args: Vec<String>,
    jsx_args: Vec<String>,
    mjs_args: Vec<String>,
    mixed_args: Vec<String>,
}

fn setup_node_check_bench() -> NodeCheckBenchSetup {
    let temp = support::ProjectTempDir::new("node-check");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    support::write_executable(
        &bin,
        "node",
        support::node_import_assertion_failure_script(),
    );
    let fixture = support::write_node_check_fixtures(temp.path());
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let js_args = strings(&["--check", &fixture.js_file.display().to_string()]);
    let js_once = node_cmd::run(&js_args).unwrap();
    assert_eq!(js_once.exit_code, 0);
    assert!(js_once.stdout.contains("plain.js"));

    let jsx_args = strings(&["--check", &fixture.jsx_file.display().to_string()]);
    let jsx_once = node_cmd::run(&jsx_args).unwrap();
    assert_eq!(jsx_once.exit_code, 0);
    assert!(jsx_once.stdout.contains("[jsx parser]"));

    let mjs_args = strings(&["--check", &fixture.mjs_file.display().to_string()]);
    let mjs_once = node_cmd::run(&mjs_args).unwrap();
    assert_eq!(mjs_once.exit_code, 1);
    assert!(mjs_once.stderr.contains("SyntaxError"));
    assert!(mjs_once.stderr.contains("'assert'"));

    let mixed_args = strings(&[
        "--check",
        &fixture.js_file.display().to_string(),
        &fixture.jsx_file.display().to_string(),
        &fixture.mjs_file.display().to_string(),
    ]);
    let mixed_once = node_cmd::run(&mixed_args).unwrap();
    assert_eq!(mixed_once.exit_code, 1);
    assert!(mixed_once.stdout.contains("plain.js"));
    assert!(mixed_once.stdout.contains("[jsx parser]"));
    assert!(mixed_once.stderr.contains("native_core.mjs"));

    NodeCheckBenchSetup {
        _env,
        _temp: temp,
        js_args,
        jsx_args,
        mjs_args,
        mixed_args,
    }
}

pub fn bench_node_check(c: &mut Criterion) {
    let setup = setup_node_check_bench();
    let mut group = c.benchmark_group("node_check");
    configure_process_group(&mut group);
    group.bench_function("js_delegated_success", |b| {
        b.iter(|| node_cmd::run(black_box(&setup.js_args)))
    });
    group.bench_function("jsx_parser_success", |b| {
        b.iter(|| node_cmd::run(black_box(&setup.jsx_args)))
    });
    group.bench_function("mjs_delegated_failure", |b| {
        b.iter(|| node_cmd::run(black_box(&setup.mjs_args)))
    });
    group.bench_function("mixed_success_and_failure", |b| {
        b.iter(|| node_cmd::run(black_box(&setup.mixed_args)))
    });
    group.finish();
}
