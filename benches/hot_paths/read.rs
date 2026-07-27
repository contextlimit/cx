use std::hint::black_box;

use criterion::Criterion;
use cx::commands::read;

use crate::hot_paths::config::configure_pure_group;
use crate::support;

pub fn bench_read(c: &mut Criterion) {
    let temp = support::ProjectTempDir::new("read");
    let source = support::rust_source_fixture(500);
    let source_path = temp.path().join("src/lib.rs");
    support::write_file(&source_path, &source);
    let huge_line_path = temp.path().join("src/blob.txt");
    support::write_file(
        &huge_line_path,
        &format!("prefix:{}:suffix\n", "A".repeat(24_000)),
    );

    let aggressive_options = read::ReadOptions {
        mode: read::ReadMode::Aggressive,
        max_lines: Some(100),
        auto_aggressive: false,
        ..read::ReadOptions::default()
    };
    let aggressive = read::run(&source_path, &aggressive_options).unwrap();
    support::assert_reduction(
        "read aggressive",
        &source,
        &aggressive.stdout,
        0.35,
        &["bench_fn_0000", "pub fn"],
    );

    let range_options = read::ReadOptions {
        line_range: Some(read::ReadRange {
            start: Some(10),
            end: Some(90),
        }),
        line_numbers: true,
        ..read::ReadOptions::default()
    };
    let raw_options = read::ReadOptions {
        raw: true,
        auto_aggressive: false,
        ..read::ReadOptions::default()
    };

    let mut group = c.benchmark_group("read");
    configure_pure_group(&mut group);
    group.bench_function("normal_medium_rust", |b| {
        b.iter(|| {
            read::run(
                black_box(&source_path),
                black_box(&read::ReadOptions::default()),
            )
        })
    });
    group.bench_function("aggressive_large_rust", |b| {
        b.iter(|| read::run(black_box(&source_path), black_box(&aggressive_options)))
    });
    group.bench_function("range_with_line_numbers", |b| {
        b.iter(|| read::run(black_box(&source_path), black_box(&range_options)))
    });
    group.bench_function("raw_huge_line", |b| {
        b.iter(|| read::run(black_box(&huge_line_path), black_box(&raw_options)))
    });
    group.finish();
}
