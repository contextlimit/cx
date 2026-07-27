#![cfg(unix)]

use criterion::{criterion_group, criterion_main};

mod hot_paths;
mod support;

criterion_group!(
    benches,
    hot_paths::cli_parse::bench_cli_parse,
    hot_paths::read::bench_read,
    hot_paths::grep::bench_grep,
    hot_paths::grep::bench_grep_fallback,
    hot_paths::insights_dashboard::bench_insights_dashboard,
    hot_paths::insights_dashboard::bench_insights_recording,
    hot_paths::runner::bench_runner,
    hot_paths::command_migrations::bench_command_migrations,
    hot_paths::wrappers::bench_wrappers,
    hot_paths::build_wrappers::bench_build_wrappers,
    hot_paths::log_wrappers::bench_log_wrappers,
    hot_paths::find::bench_find,
    hot_paths::node_check::bench_node_check,
    hot_paths::process_inventory::bench_process_inventory
);
criterion_main!(benches);
