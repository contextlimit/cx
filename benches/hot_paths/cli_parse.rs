use std::hint::black_box;

use criterion::Criterion;
use cx::cli::{try_parse_from_cx_args, Command};

use crate::hot_paths::config::configure_process_group;

const STANDARD_RG_ARGS: [&str; 6] = [
    "cx",
    "rg",
    "-n",
    "DispatchVoiceContractCommandJson",
    "src",
    "--hidden",
];
const REPEATED_RG_ARGS: [&str; 8] = [
    "cx",
    "rg",
    "-n",
    "DispatchVoiceContractCommandJson",
    "-n",
    "src",
    "--hidden",
    "--hidden",
];

pub fn bench_cli_parse(c: &mut Criterion) {
    assert_rg_parse(&STANDARD_RG_ARGS);
    assert_rg_parse(&REPEATED_RG_ARGS);

    let mut group = c.benchmark_group("cli_parse");
    configure_process_group(&mut group);
    group.bench_function("rg_standard_flags", |b| {
        b.iter(|| try_parse_from_cx_args(black_box(STANDARD_RG_ARGS)))
    });
    group.bench_function("rg_repeated_boolean_flags", |b| {
        b.iter(|| try_parse_from_cx_args(black_box(REPEATED_RG_ARGS)))
    });
    group.finish();
}

fn assert_rg_parse(args: &[&str]) {
    let parsed = try_parse_from_cx_args(args.iter().copied()).unwrap();
    let Command::Grep {
        line_numbers,
        hidden,
        extended_regexp,
        ..
    } = parsed.command
    else {
        panic!("expected grep command");
    };
    assert!(line_numbers);
    assert!(hidden);
    assert!(extended_regexp);
}
