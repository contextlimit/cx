use std::env;
use std::ffi::OsString;
use std::hint::black_box;

use criterion::Criterion;
use cx::commands::insights::run_dashboard;
use cx::support::insights::{
    record_command_opportunity, record_command_report, record_invocation, record_routing_rejection,
    CommandFilter, CommandOpportunityRecord, CommandReportRecord, InvocationRecord,
    OpportunityConfidence, OutputObservation, RoutingDecisionRecord, TextMetrics,
};

use crate::hot_paths::config::configure_process_group;
use crate::support;

struct DashboardBenchSetup {
    _env: DashboardEnvGuard,
    _temp: support::ProjectTempDir,
}

struct RecordingBenchSetup {
    _env: DashboardEnvGuard,
    _temp: support::ProjectTempDir,
}

const RECORDING_HISTORY_ROWS: u64 = 100_000;

fn setup_dashboard_bench() -> DashboardBenchSetup {
    let temp = support::ProjectTempDir::new("insights-dashboard");
    let db_path = temp.path().join("db.sqlite");
    let home = temp.path().join("home");
    let env = DashboardEnvGuard::set(&db_path, &home);

    for index in 0..600 {
        let command = match index % 4 {
            0 => "git diff",
            1 => "grep",
            2 => "read",
            _ => "cargo test",
        };
        let raw = OutputObservation::from_text(command, &"fixture evidence\n".repeat(60));
        record_invocation(&InvocationRecord {
            command,
            exit_code: 0,
            raw: Some(&raw),
            emitted: TextMetrics::from_text("fixture evidence\n"),
        })
        .unwrap();
    }
    record_command_report(&CommandReportRecord {
        command: "cx grep fixture src",
        command_family: "grep",
        command_shape: "",
        command_shape_hash: "",
        issue_kind: "suspicious_output",
        note: "benchmark quality report",
    })
    .unwrap();
    record_command_opportunity(&CommandOpportunityRecord {
        process: "passthrough",
        command_family: "passthrough jq",
        command: "jq benchmark",
        source: "jq",
        strategy: "bounded-head-tail",
        confidence: OpportunityConfidence::Low,
        raw: TextMetrics::from_text(&"json row\n".repeat(500)),
        projected: TextMetrics::from_text("json row\n... [480 lines omitted] ...\njson row\n"),
    })
    .unwrap();
    for index in 0..100 {
        let args = vec![
            "cx".to_string(),
            "--".to_string(),
            "git".to_string(),
            "branch".to_string(),
            format!("shape-{index}"),
        ];
        record_routing_rejection(&RoutingDecisionRecord {
            args: &args,
            reason: "passthrough-disabled",
            error_kind: "invalid-subcommand",
            explicit_auto: true,
            passthrough_eligible: true,
            passthrough_enabled: false,
        })
        .unwrap();
    }

    let output = run_dashboard(25, CommandFilter::default()).unwrap();
    assert_dashboard_bench_output(&output.stdout);

    DashboardBenchSetup {
        _env: env,
        _temp: temp,
    }
}

fn assert_dashboard_bench_output(output: &str) {
    let value: serde_json::Value = serde_json::from_str(output).unwrap();
    assert_eq!(value["schema_version"], 12);
    assert_eq!(value["source_export_schema_version"], 18);
    assert_eq!(value["summary"]["invocations"], 600);
    assert_eq!(value["savings_distribution"]["invocations"], 600);
    assert!(
        value["savings_distribution"]["concentration"]["saved_tokens_excluding_top_10"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        value["tables"]["recent_invocations"]
            .as_array()
            .unwrap()
            .len()
            <= 25
    );
    assert!(
        value["tables"]["largest_invocations"]
            .as_array()
            .unwrap()
            .len()
            <= 25
    );
    assert!(value["tables"]["recent_invocations"][0]["raw"]["tokens"].is_u64());
    assert_eq!(
        value["tables"]["passthrough_opportunities"][0]["estimate"],
        true
    );
    assert_eq!(value["health"]["routing_summary"]["rejections"], 100);
    assert!(
        value["tables"]["recent_routing_decisions"]
            .as_array()
            .unwrap()
            .len()
            <= 25
    );
}

pub fn bench_insights_dashboard(c: &mut Criterion) {
    let _setup = setup_dashboard_bench();
    let mut group = c.benchmark_group("insights_dashboard");
    configure_process_group(&mut group);
    group.bench_function("ui_snapshot_600_rows", |b| {
        b.iter(|| run_dashboard(black_box(25), black_box(CommandFilter::default())))
    });
    group.finish();
}

fn setup_recording_bench() -> RecordingBenchSetup {
    let temp = support::ProjectTempDir::new("insights-recording");
    let db_path = temp.path().join("db.sqlite");
    let home = temp.path().join("home");
    let env = DashboardEnvGuard::set(&db_path, &home);
    record_invocation(&InvocationRecord {
        command: "recording benchmark seed",
        exit_code: 0,
        raw: None,
        emitted: TextMetrics::from_text("seed\n"),
    })
    .unwrap();

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute(
            "
            WITH digits(value) AS (
                VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9)
            ), history(row_number) AS (
                SELECT ones.value
                     + tens.value * 10
                     + hundreds.value * 100
                     + thousands.value * 1000
                     + ten_thousands.value * 10000
                FROM digits AS ones
                CROSS JOIN digits AS tens
                CROSS JOIN digits AS hundreds
                CROSS JOIN digits AS thousands
                CROSS JOIN digits AS ten_thousands
            )
            INSERT INTO command_invocations (
                occurred_at_ms, command, source, exit_code,
                raw_bytes, raw_chars, raw_lines, raw_tokens,
                emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
                saved_bytes, saved_chars, saved_lines, saved_tokens,
                savings_ratio, compression_ratio
            )
            SELECT row_number, 'historical command', 'benchmark fixture', 0,
                   64, 64, 4, 16, 16, 16, 1, 4, 48, 48, 3, 12, 0.75, 0.25
            FROM history
            WHERE row_number < ?1
            ",
            [RECORDING_HISTORY_ROWS],
        )
        .unwrap();
    let rows: u64 = connection
        .query_row("SELECT COUNT(*) FROM command_invocations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, RECORDING_HISTORY_ROWS + 1);

    RecordingBenchSetup {
        _env: env,
        _temp: temp,
    }
}

pub fn bench_insights_recording(c: &mut Criterion) {
    let _setup = setup_recording_bench();
    let raw = OutputObservation::from_text("benchmark native output", &"evidence\n".repeat(64));
    let record = InvocationRecord {
        command: "steady state recording",
        exit_code: 0,
        raw: Some(&raw),
        emitted: TextMetrics::from_text("evidence\n"),
    };
    let mut group = c.benchmark_group("insights_recording");
    configure_process_group(&mut group);
    group.bench_function("steady_state_write_100k_history", |b| {
        b.iter(|| record_invocation(black_box(&record)).unwrap())
    });
    group.finish();
}

struct DashboardEnvGuard {
    previous_db: Option<OsString>,
    previous_home: Option<OsString>,
    previous_disable: Option<OsString>,
}

impl DashboardEnvGuard {
    fn set(db_path: &std::path::Path, home: &std::path::Path) -> Self {
        let guard = Self {
            previous_db: env::var_os("CX_INSIGHTS_DB_PATH"),
            previous_home: env::var_os("HOME"),
            previous_disable: env::var_os("CX_DISABLE_INSIGHTS"),
        };
        env::set_var("CX_INSIGHTS_DB_PATH", db_path);
        env::set_var("HOME", home);
        env::remove_var("CX_DISABLE_INSIGHTS");
        guard
    }
}

impl Drop for DashboardEnvGuard {
    fn drop(&mut self) {
        restore_env("CX_INSIGHTS_DB_PATH", self.previous_db.take());
        restore_env("HOME", self.previous_home.take());
        restore_env("CX_DISABLE_INSIGHTS", self.previous_disable.take());
    }
}

fn restore_env(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => env::set_var(name, value),
        None => env::remove_var(name),
    }
}
