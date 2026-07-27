use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;

fn cx_command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
    command
        .current_dir(home)
        .env("HOME", home)
        .env_remove("CX_DISABLE_INSIGHTS")
        .env_remove("CX_ENABLE_INSIGHTS")
        .env_remove("CX_INSIGHTS_DB_PATH")
        .env_remove("CX_ENABLE_UNSUPPORTED_PASSTHROUGH")
        .env_remove("CX_INSIGHTS_DEBUG");
    command
}

fn run(home: &Path, args: &[&str]) -> Output {
    cx_command(home).args(args).output().unwrap()
}

fn database_path(home: &Path) -> PathBuf {
    home.join(".cx/db.sqlite")
}

fn enable_recording(home: &Path, command_text: bool) {
    let command_text = if command_text { "true" } else { "false" };
    let output = run(
        home,
        &[
            "insights",
            "settings",
            "--set",
            "record_invocations=true",
            "--set",
            &format!("record_command_text={command_text}"),
            "--set",
            "record_command_shape=true",
            "--set",
            "passthrough_unsupported_commands=false",
        ],
    );
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn rejected_route_does_not_create_default_database_when_recording_is_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let output = run(
        &home,
        &["--", "read", "--head", "1", "--tail", "1", "fixture.rs"],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(!database_path(&home).exists());
}

#[test]
fn cx_owned_parse_rejection_records_bounded_redacted_command_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    enable_recording(&home, true);

    let secret = "sk-abcdefghijklmnopqrstuvwxyz0123456789";
    let output = cx_command(&home)
        .args(["--", "read", "--head", "1", "--tail", "1", secret])
        .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    let connection = Connection::open(database_path(&home)).unwrap();
    let row = connection
        .query_row(
            "SELECT process, command_family, command, argv_json, command_shape, \
                    command_shape_hash, decision, reason, error_kind, explicit_auto, \
                    passthrough_eligible, passthrough_enabled \
             FROM command_routing_decisions",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row.0, "read");
    assert_eq!(row.1, "read");
    assert_eq!(row.6, "rejected");
    assert_eq!(row.7, "cx-owned-parse-error");
    assert_eq!(row.8, "argument-conflict");
    assert_eq!((row.9, row.10, row.11), (1, 0, 1));
    assert!(!row.2.contains(secret));
    assert!(!row.3.contains(secret));
    assert!(!row.4.contains(secret));
    assert!(!row.5.is_empty());
    assert!(serde_json::from_str::<Vec<String>>(&row.3).is_ok());

    let routing = run(&home, &["insights", "routing", "--limit", "5"]);
    assert!(routing.status.success(), "{routing:?}");
    let routing_stdout = String::from_utf8(routing.stdout).unwrap();
    assert!(routing_stdout.contains("Rejected: 1"));
    assert!(routing_stdout.contains("cx-owned-parse-error"));

    let export = run(
        &home,
        &["insights", "export", "--format", "json", "--limit", "5"],
    );
    assert!(export.status.success(), "{export:?}");
    let export_json: serde_json::Value = serde_json::from_slice(&export.stdout).unwrap();
    assert_eq!(export_json["schema_version"], 18);
    assert_eq!(export_json["no_data"], true);
    assert_eq!(export_json["routing_summary"]["rejections"], 1);

    let dashboard = run(&home, &["insights", "dashboard", "--limit", "5"]);
    assert!(dashboard.status.success(), "{dashboard:?}");
    let dashboard_json: serde_json::Value = serde_json::from_slice(&dashboard.stdout).unwrap();
    assert_eq!(dashboard_json["schema_version"], 12);
    assert_eq!(dashboard_json["empty_state"]["no_invocations"], true);
    assert_eq!(
        dashboard_json["empty_state"]["no_routing_rejections"],
        false
    );
    assert_eq!(
        dashboard_json["tables"]["recent_routing_decisions"][0]["error_kind"],
        "argument-conflict"
    );
}

#[test]
fn passthrough_disabled_rejection_keeps_command_text_optional() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    enable_recording(&home, false);

    let output = run(&home, &["--", "git", "branch", "--show-current"]);
    assert_eq!(output.status.code(), Some(2));

    let connection = Connection::open(database_path(&home)).unwrap();
    let row = connection
        .query_row(
            "SELECT process, command_family, command, argv_json, command_shape, \
                    reason, error_kind, passthrough_eligible, passthrough_enabled \
             FROM command_routing_decisions",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row.0, "git");
    assert_eq!(row.1, "git branch");
    assert_eq!(row.2, "git branch");
    assert_eq!(row.3, "[]");
    assert!(!row.4.is_empty());
    assert_eq!(row.5, "passthrough-disabled");
    assert_eq!(row.6, "invalid-subcommand");
    assert_eq!((row.7, row.8), (1, 0));

    let version: String = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'insights_schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "19");
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal");
}
