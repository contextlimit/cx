use std::hint::black_box;

use criterion::Criterion;
use cx::commands::ps_cmd;

use crate::hot_paths::config::configure_process_group;
use crate::support;

struct ProcessInventoryBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    args: Vec<String>,
}

fn setup_process_inventory_bench() -> ProcessInventoryBenchSetup {
    let temp = support::ProjectTempDir::new("process-inventory");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw = support::ps_output_fixture(720);
    let output_path = temp.path().join("ps.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "ps",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);
    let args = vec!["-axo".to_string(), "pid,ppid,etime,command".to_string()];

    let outcome = ps_cmd::run(&args).unwrap();
    support::assert_reduction(
        "ps inventory benchmark",
        &raw,
        &outcome.stdout,
        0.08,
        &[
            "ps: 720 processes across 3 executables",
            "node |",
            "python3 |",
            "rare-service | 1",
            "[full process table:",
        ],
    );

    ProcessInventoryBenchSetup {
        _env,
        _temp: temp,
        args,
    }
}

pub fn bench_process_inventory(c: &mut Criterion) {
    let setup = setup_process_inventory_bench();
    let mut group = c.benchmark_group("process_inventory");
    configure_process_group(&mut group);
    group.bench_function("ps_broad_inventory", |b| {
        b.iter(|| ps_cmd::run(black_box(&setup.args)))
    });
    group.finish();
}
