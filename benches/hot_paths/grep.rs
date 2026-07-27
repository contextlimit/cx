use std::hint::black_box;

use criterion::Criterion;
use cx::commands::grep;

use crate::hot_paths::config::configure_process_group;
use crate::support;

struct GrepBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    small_options: grep::GrepOptions,
    fixed_options: grep::GrepOptions,
    extended_options: grep::GrepOptions,
    raw_options: grep::GrepOptions,
    list_options: grep::GrepOptions,
}

fn setup_grep_bench() -> GrepBenchSetup {
    let temp = support::ProjectTempDir::new("grep");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw_output = support::grep_output_fixture(96);
    let large_raw_output = support::grep_output_fixture(1_200);
    let small_raw_output = "src/archive.rs:330:        let rawBytes = metrics.raw_bytes;\n";
    let file_list_output = support::file_list_fixture(180);
    let output_path = temp.path().join("rg-output.txt");
    let large_output_path = temp.path().join("rg-large-output.txt");
    let small_output_path = temp.path().join("rg-small-output.txt");
    let file_list_path = temp.path().join("rg-files.txt");
    support::write_file(&output_path, &raw_output);
    support::write_file(&large_output_path, &large_raw_output);
    support::write_file(&small_output_path, small_raw_output);
    support::write_file(&file_list_path, &file_list_output);
    support::write_executable(
        &bin,
        "rg",
        &format!(
            "#!/bin/sh\ncase \" $* \" in\n  *\" --files \"*) /bin/cat {} ;;\n  *\" nomatch \"*) exit 1 ;;\n  *\" many_needle \"*) /bin/cat {} ;;\n  *\" small_exact \"*) /bin/cat {} ;;\n  *) /bin/cat {} ;;\nesac\n",
            support::shell_quote(&file_list_path),
            support::shell_quote(&large_output_path),
            support::shell_quote(&small_output_path),
            support::shell_quote(&output_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let fixed_options = grep::GrepOptions {
        fixed_strings: true,
        max_results: Some(10),
        ..grep::GrepOptions::default()
    };
    let fixed = grep::run_many(
        &["needle".to_string(), "route_state".to_string()],
        &["src".to_string()],
        &fixed_options,
    )
    .unwrap();
    support::assert_reduction(
        "grep fixed",
        &raw_output,
        &fixed.stdout,
        0.45,
        &["10 shown of 96 matches", "src/module_00.rs"],
    );

    let small_options = grep::GrepOptions {
        fixed_strings: true,
        ..grep::GrepOptions::default()
    };
    let small = grep::run(
        "small_exact",
        &["src/archive.rs".to_string()],
        &small_options,
    )
    .unwrap();
    assert_eq!(small.stdout, small_raw_output.trim_end());

    let many = grep::run_many(
        &["many_needle".to_string()],
        &["src".to_string()],
        &fixed_options,
    )
    .unwrap();
    support::assert_reduction(
        "grep many",
        &large_raw_output,
        &many.stdout,
        0.05,
        &["10 shown of 1200 matches", "src/module_00.rs"],
    );

    let extended_options = grep::GrepOptions {
        extended_regexp: true,
        max_results: Some(12),
        ..grep::GrepOptions::default()
    };
    let raw_options = grep::GrepOptions {
        context_lines: Some(2),
        ..grep::GrepOptions::default()
    };
    let list_options = grep::GrepOptions {
        glob_patterns: vec!["*.rs".to_string()],
        max_results: Some(20),
        ..grep::GrepOptions::default()
    };
    let file_list = grep::list_files(&["src".to_string()], &list_options).unwrap();
    support::assert_reduction(
        "rg files",
        &file_list_output,
        &file_list.stdout,
        0.35,
        &["20 shown of 180 files", "src/module_000.rs"],
    );

    GrepBenchSetup {
        _env,
        _temp: temp,
        small_options,
        fixed_options,
        extended_options,
        raw_options,
        list_options,
    }
}

pub fn bench_grep(c: &mut Criterion) {
    let setup = setup_grep_bench();
    let mut group = c.benchmark_group("grep_rg");
    configure_process_group(&mut group);
    group.bench_function("small_exact_result", |b| {
        b.iter(|| {
            grep::run(
                black_box("small_exact"),
                black_box(&["src/archive.rs".to_string()]),
                black_box(&setup.small_options),
            )
        })
    });
    group.bench_function("fixed_multi_pattern", |b| {
        b.iter(|| {
            grep::run_many(
                black_box(&["needle".to_string(), "route_state".to_string()]),
                black_box(&["src".to_string()]),
                black_box(&setup.fixed_options),
            )
        })
    });
    group.bench_function("many_matches_max_results", |b| {
        b.iter(|| {
            grep::run_many(
                black_box(&["many_needle".to_string()]),
                black_box(&["src".to_string()]),
                black_box(&setup.fixed_options),
            )
        })
    });
    group.bench_function("extended_regex", |b| {
        b.iter(|| {
            grep::run_many(
                black_box(&["needle_[0-9]+|route_state".to_string()]),
                black_box(&["src".to_string()]),
                black_box(&setup.extended_options),
            )
        })
    });
    group.bench_function("raw_context_fallback", |b| {
        b.iter(|| {
            grep::run_many(
                black_box(&["needle".to_string()]),
                black_box(&["src".to_string()]),
                black_box(&setup.raw_options),
            )
        })
    });
    group.bench_function("no_matches", |b| {
        b.iter(|| {
            grep::run_many(
                black_box(&["nomatch".to_string()]),
                black_box(&["src".to_string()]),
                black_box(&grep::GrepOptions::default()),
            )
        })
    });
    group.bench_function("list_files_rg_files", |b| {
        b.iter(|| {
            grep::list_files(
                black_box(&["src".to_string()]),
                black_box(&setup.list_options),
            )
        })
    });
    group.finish();
}

struct GrepFallbackBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    options: grep::GrepOptions,
}

fn setup_grep_fallback_bench() -> GrepFallbackBenchSetup {
    let temp = support::ProjectTempDir::new("grep-fallback");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw_output = support::grep_output_fixture(120);
    let output_path = temp.path().join("grep-output.txt");
    support::write_file(&output_path, &raw_output);
    support::write_executable(
        &bin,
        "grep",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let options = grep::GrepOptions {
        fixed_strings: true,
        max_results: Some(10),
        ..grep::GrepOptions::default()
    };
    let fallback = grep::run_many(
        &["needle".to_string(), "route_state".to_string()],
        &["src".to_string()],
        &options,
    )
    .unwrap();
    support::assert_reduction(
        "grep fallback",
        &raw_output,
        &fallback.stdout,
        0.45,
        &["10 shown of 120 matches", "src/module_00.rs"],
    );

    GrepFallbackBenchSetup {
        _env,
        _temp: temp,
        options,
    }
}

pub fn bench_grep_fallback(c: &mut Criterion) {
    let setup = setup_grep_fallback_bench();
    let mut group = c.benchmark_group("grep_fallback");
    configure_process_group(&mut group);
    group.bench_function("fixed_multi_pattern", |b| {
        b.iter(|| {
            grep::run_many(
                black_box(&["needle".to_string(), "route_state".to_string()]),
                black_box(&["src".to_string()]),
                black_box(&setup.options),
            )
        })
    });
    group.finish();
}
