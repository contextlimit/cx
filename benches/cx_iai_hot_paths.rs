#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::hint::black_box;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use cx::commands::read::{self, ReadMode, ReadOptions, ReadRange};
#[cfg(target_os = "linux")]
use cx::support::runner::ProxyOutcome;
#[cfg(target_os = "linux")]
use iai_callgrind::{library_benchmark, library_benchmark_group, main};

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::medium_rust(setup_rust_source_path())]
fn read_normal_medium_rust(path: PathBuf) -> ProxyOutcome {
    let options = ReadOptions::default();
    read::run(black_box(path.as_path()), black_box(&options)).unwrap()
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::large_rust(setup_rust_source_path())]
fn read_aggressive_large_rust(path: PathBuf) -> ProxyOutcome {
    let options = ReadOptions {
        mode: ReadMode::Aggressive,
        max_lines: Some(100),
        auto_aggressive: false,
        ..ReadOptions::default()
    };
    read::run(black_box(path.as_path()), black_box(&options)).unwrap()
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::range_numbered(setup_rust_source_path())]
fn read_range_with_line_numbers(path: PathBuf) -> ProxyOutcome {
    let options = ReadOptions {
        line_range: Some(ReadRange {
            start: Some(10),
            end: Some(90),
        }),
        line_numbers: true,
        ..ReadOptions::default()
    };
    read::run(black_box(path.as_path()), black_box(&options)).unwrap()
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::huge_line(setup_huge_line_path())]
fn read_raw_huge_line(path: PathBuf) -> ProxyOutcome {
    let options = ReadOptions {
        raw: true,
        auto_aggressive: false,
        ..ReadOptions::default()
    };
    read::run(black_box(path.as_path()), black_box(&options)).unwrap()
}

#[cfg(target_os = "linux")]
library_benchmark_group!(
    name = read_hot_paths;
    benchmarks =
        read_normal_medium_rust,
        read_aggressive_large_rust,
        read_range_with_line_numbers,
        read_raw_huge_line
);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = read_hot_paths);

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("iai-callgrind benchmarks are Linux-only; skipping on this target");
}

#[cfg(target_os = "linux")]
fn setup_rust_source_path() -> PathBuf {
    let path = fixture_root().join("src/lib.rs");
    write_fixture_if_needed(&path, &rust_source_fixture(500));
    path
}

#[cfg(target_os = "linux")]
fn setup_huge_line_path() -> PathBuf {
    let path = fixture_root().join("src/blob.txt");
    write_fixture_if_needed(&path, &format!("prefix:{}:suffix\n", "A".repeat(24_000)));
    path
}

#[cfg(target_os = "linux")]
fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".tmp")
        .join("cx-iai-hot-paths")
}

#[cfg(target_os = "linux")]
fn write_fixture_if_needed(path: &Path, content: &str) {
    if fs::read_to_string(path).is_ok_and(|existing| existing == content) {
        return;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create IAI fixture parent");
    }
    fs::write(path, content).expect("write IAI fixture");
}

#[cfg(target_os = "linux")]
fn rust_source_fixture(functions: usize) -> String {
    let mut content = String::new();
    for index in 0..functions {
        content.push_str(&format!("/// Documentation for bench_fn_{index:04}\n"));
        content.push_str("#[inline]\n");
        content.push_str(&format!(
            "pub fn bench_fn_{index:04}(value: usize) -> usize {{\n"
        ));
        content.push_str("    let mut acc = value;\n");
        content.push_str("    acc = acc.wrapping_mul(31).wrapping_add(7);\n");
        content.push_str("    acc\n");
        content.push_str("}\n\n");
    }
    content
}
