use std::hint::black_box;

use criterion::Criterion;
use cx::commands::find;

use crate::hot_paths::config::configure_pure_group;
use crate::hot_paths::util::strings;
use crate::support;

struct FindBenchSetup {
    _temp: support::ProjectTempDir,
    name_args: Vec<String>,
    path_args: Vec<String>,
    perm_args: Vec<String>,
}

fn setup_find_bench() -> FindBenchSetup {
    let temp = support::ProjectTempDir::new("find");
    let fixture = support::populate_find_fixture(temp.path(), 360);
    let root = fixture.root.display().to_string();
    let name_args = strings(&[
        &root,
        "-maxdepth",
        "4",
        "-type",
        "f",
        "-name",
        "*.rs",
        "--max-results",
        "30",
    ]);
    let name_once = find::run(&name_args).unwrap();
    support::assert_reduction(
        "find name benchmark",
        &fixture.raw_rs_listing,
        &name_once.stdout,
        0.15,
        &["30 shown of 360 entries", "file_0000.rs"],
    );

    let path_args = strings(&[
        &root,
        "-maxdepth",
        "5",
        "-type",
        "d",
        "-path",
        "*/node_modules/playwright",
    ]);
    let path_once = find::run(&path_args).unwrap();
    assert!(path_once.stdout.contains(&fixture.path_dir_name));

    let perm_args = strings(&[&root, "-type", "f", "-perm", "-111"]);
    let perm_once = find::run(&perm_args).unwrap();
    assert!(perm_once.stdout.contains(&fixture.executable_name));

    FindBenchSetup {
        _temp: temp,
        name_args,
        path_args,
        perm_args,
    }
}

pub fn bench_find(c: &mut Criterion) {
    let setup = setup_find_bench();
    let mut group = c.benchmark_group("find");
    configure_pure_group(&mut group);
    group.bench_function("name_type_maxdepth", |b| {
        b.iter(|| find::run(black_box(&setup.name_args)))
    });
    group.bench_function("path_predicate", |b| {
        b.iter(|| find::run(black_box(&setup.path_args)))
    });
    group.bench_function("perm_executable", |b| {
        b.iter(|| find::run(black_box(&setup.perm_args)))
    });
    group.finish();
}
