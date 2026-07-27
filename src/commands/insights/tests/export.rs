use super::*;
use crate::support::insights::{
    record_failure_artifact, FailureArtifactRecord, OpportunityConfidence,
};

#[test]
fn run_export_json_renders_seeded_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let home = temp.path().join("home");
    let artifact_dir = home.join(".cx/cache/failures/grep");
    fs::create_dir_all(&artifact_dir).unwrap();
    fs::write(artifact_dir.join("001.log"), "grep failure").unwrap();
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            seed_export_fixture();
            let output = run_export(ExportFormat::Json, 3, CommandFilter::default()).unwrap();
            let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
            assert_export_json_snapshot(&value, &db_path);
        },
    );
}

#[test]
fn run_export_json_can_filter_by_root_and_records_filter_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let diff_raw = OutputObservation::from_text("git diff", &"one\n".repeat(20));
            record_invocation(&InvocationRecord {
                command: "git diff",
                exit_code: 0,
                raw: Some(&diff_raw),
                emitted: TextMetrics::from_text("one\n"),
            })
            .unwrap();
            let read_raw = OutputObservation::from_text("read source", &"one\n".repeat(20));
            record_invocation(&InvocationRecord {
                command: "read",
                exit_code: 0,
                raw: Some(&read_raw),
                emitted: TextMetrics::from_text("one\n"),
            })
            .unwrap();

            let output = run_export(
                ExportFormat::Json,
                5,
                CommandFilter {
                    command_root: Some("git"),
                    command: None,
                },
            )
            .unwrap();
            let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(value["filter"]["active"], true);
            assert_eq!(value["filter"]["command_root"], "git");
            assert_eq!(value["overall"]["invocations"], 1);
            assert_eq!(value["savings_distribution"]["invocations"], 1);
            assert!(value["recent_invocations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["process"] == "git"));
        },
    );
}

#[test]
fn run_export_includes_expansion_metrics_and_reason() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            let raw = OutputObservation::from_text("git status", "")
                .with_expansion_reason("status-summary");
            record_invocation(&InvocationRecord {
                command: "git status",
                exit_code: 0,
                raw: Some(&raw),
                emitted: TextMetrics::from_text("Clean working tree"),
            })
            .unwrap();

            let json = run_export(ExportFormat::Json, 3, CommandFilter::default()).unwrap();
            let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
            assert_eq!(value["overall"]["expansions"], 1);
            assert!(value["overall"]["expanded"]["tokens"].as_u64().unwrap() > 0);
            assert!(value["overall"]["net_token_delta"].as_i64().unwrap() > 0);
            assert_eq!(value["presentation"]["metrics"]["expansions"], 1);
            assert!(
                value["presentation"]["metrics"]["net_token_delta"]
                    .as_i64()
                    .unwrap()
                    > 0
            );
            assert_eq!(
                value["recent_invocations"][0]["expansion_reason"],
                "status-summary"
            );
            assert!(
                value["recent_invocations"][0]["expanded"]["tokens"]
                    .as_u64()
                    .unwrap()
                    > 0
            );

            let csv = run_export(ExportFormat::Csv, 3, CommandFilter::default()).unwrap();
            assert!(csv.stdout.contains("overall,,expansions,1"));
            assert!(csv.stdout.contains("presentation_metrics,,expansions,1"));
            assert!(csv.stdout.contains("expansion_reason,status-summary"));
        },
    );
}

#[test]
fn run_export_csv_renders_long_form_rows() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let home = temp.path().join("home");
    let artifact_dir = home.join(".cx/cache/failures/grep");
    fs::create_dir_all(&artifact_dir).unwrap();
    fs::write(artifact_dir.join("001.log"), "grep failure").unwrap();
    crate::support::test_support::with_env_vars(
        &[
            ("HOME", Some(home.to_string_lossy().as_ref())),
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            seed_export_fixture();
            let output = run_export(ExportFormat::Csv, 3, CommandFilter::default()).unwrap();
            assert_export_csv_snapshot(&output.stdout, &db_path);
        },
    );
}

fn assert_export_csv_snapshot(output: &str, db_path: &Path) {
    assert!(output.starts_with(
        "section,rank,metric,value,process,command_family,command,day,invocation_id,source,exit_code,argv_json,command_shape,command_shape_hash\n"
    ));
    for expected in [
        "top_roots,1,saved_tokens",
        "top_commands,1,saved_tokens",
        "largest_invocations,1,saved_tokens",
        "recent_invocations,1,saved_tokens",
        "daily_totals,1,saved_tokens",
        "metadata,,schema_name,cx-insights-export",
        "metadata,,schema_version,18",
        "metadata,,generated_at_ms,",
        "metadata,,limit,3",
        "metadata,,filter_active,false",
        "presentation,1,headline",
        "presentation_metrics,,invocations,2",
        "presentation_metrics,,expansions,0",
        "presentation_metrics,,failure_rate,0.500000",
        "presentation_metrics,,saved_tokens",
        "savings_distribution,,all_p50_saved_tokens",
        "savings_distribution,,saving_p95_saved_tokens",
        "savings_distribution,,top_10_share,1.000000",
        "savings_distribution,,saved_tokens_excluding_top_10,0",
        "recommendations,1,title",
        "command_report_totals,1,reports,1,grep,,grep",
        "command_report_totals,1,open,1,grep,,grep",
        "recent_command_reports,1,issue_kind,suspicious_output,grep,grep,grep",
        "recent_command_reports,1,status,open,grep,grep,grep",
        "recent_command_reports,1,evidence_kind,no-match,grep,grep,grep",
        "recent_command_reports,1,cx_response_recorded,0,grep,grep,grep",
        "recent_command_reports,1,native_response_recorded,0,grep,grep,grep",
        "operational_health,,missing_artifact_risks,0",
        "operational_health,,unknown_failure_invocations,1",
        "operational_health,,families_with_retained_artifacts,1",
        "failure_focus,1,artifact_count,1,grep,,grep",
        "passthrough_opportunities,1,samples,1,passthrough,passthrough seq,passthrough seq",
        "passthrough_opportunities,1,strategy,test-generic,passthrough,passthrough seq,passthrough seq",
        "passthrough_opportunities,1,potential_saved_tokens",
        "routing_summary,,rejections,1",
        "routing_decision_totals,1,decisions,1,git,git branch",
        "recent_routing_decisions,1,reason,passthrough-disabled,git,git branch",
    ] {
        assert!(output.contains(expected), "missing CSV evidence: {expected}");
    }
    assert!(output.contains("\"git,diff\""));
    assert!(output.contains(&format!("metadata,,database,{}", db_path.to_string_lossy())));
}

fn seed_export_fixture() {
    let raw = OutputObservation::from_text("git,diff", &"one\n".repeat(80));
    record_invocation(&InvocationRecord {
        command: "git diff",
        exit_code: 0,
        raw: Some(&raw),
        emitted: TextMetrics::from_text("one\n"),
    })
    .unwrap();
    let grep_raw = OutputObservation::from_text("grep", &"match\n".repeat(12));
    record_invocation(&InvocationRecord {
        command: "grep",
        exit_code: 2,
        raw: Some(&grep_raw),
        emitted: TextMetrics::from_text("match\n"),
    })
    .unwrap();
    record_command_report(&CommandReportRecord {
        command: "cx grep route|path src",
        command_family: "grep",
        command_shape: "",
        command_shape_hash: "",
        issue_kind: "suspicious_output",
        note: "bare alternation returned zero matches",
    })
    .unwrap();
    record_failure_artifact(&FailureArtifactRecord {
        display_path: "~/.cx/cache/failures/grep/001.log",
        tool_name: "grep",
        compression: "none",
        stdout_bytes: 12,
        stderr_bytes: 4,
        original_bytes: 16,
        stored_bytes: 38,
    })
    .unwrap();
    record_command_opportunity(&CommandOpportunityRecord {
        process: "passthrough",
        command_family: "passthrough seq",
        command: "seq 1 120",
        source: "passthrough:seq",
        strategy: "test-generic",
        confidence: OpportunityConfidence::Low,
        raw: TextMetrics::from_text(&"row\n".repeat(120)),
        projected: TextMetrics::from_text("row\n... [80 lines omitted] ...\nrow\n"),
    })
    .unwrap();
    record_routing_rejection(&RoutingDecisionRecord {
        args: &[
            "cx".to_string(),
            "--".to_string(),
            "git".to_string(),
            "branch".to_string(),
            "--show-current".to_string(),
        ],
        reason: "passthrough-disabled",
        error_kind: "invalid-subcommand",
        explicit_auto: true,
        passthrough_eligible: true,
        passthrough_enabled: false,
    })
    .unwrap();
}

fn assert_export_json_snapshot(value: &serde_json::Value, db_path: &Path) {
    assert_export_json_metadata(value, db_path);
    assert_export_json_invocation_sections(value);
    assert_export_json_presentation_sections(value);
    assert_export_json_report_sections(value);
    assert_export_json_opportunity_sections(value);
    assert_export_json_routing_sections(value);
}

fn assert_export_json_metadata(value: &serde_json::Value, db_path: &Path) {
    assert_eq!(value["schema_name"], EXPORT_SCHEMA_NAME);
    assert_eq!(value["schema_version"], EXPORT_SCHEMA_VERSION);
    assert!(value["generated_at_ms"].as_u64().unwrap() > 0);
    assert_eq!(value["limit"], 3);
    assert_eq!(
        value["database"].as_str().unwrap(),
        db_path.to_string_lossy()
    );
    assert_eq!(value["no_data"], false);
    assert_eq!(value["filter"]["active"], false);
    assert_eq!(value["overall"]["invocations"], 2);
    assert_eq!(value["savings_distribution"]["invocations"], 2);
    assert_eq!(value["savings_distribution"]["saving_invocations"], 2);
    assert!(
        value["savings_distribution"]["percentiles"]["all_invocations"]["p50_saved_tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        value["savings_distribution"]["concentration"]["top_10_share"],
        1.0
    );
}

fn assert_export_json_invocation_sections(value: &serde_json::Value) {
    assert_eq!(value["top_roots"][0]["command"], "git");
    assert_eq!(value["top_commands"][0]["command"], "git diff");
    assert_eq!(value["largest_invocations"][0]["source"], "git,diff");
    assert_eq!(value["largest_invocations"][0]["process"], "git");
    assert_eq!(
        value["largest_invocations"][0]["command_family"],
        "git diff"
    );
    assert_eq!(
        value["largest_invocations"][0]["argv"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(value["recent_invocations"][0]["command"], "grep");
    assert_eq!(value["recent_invocations"][0]["process"], "grep");
    assert!(!value["recent_invocations"][0]["binary_version"]
        .as_str()
        .unwrap()
        .is_empty());
    assert!(!value["recent_invocations"][0]["binary_revision"]
        .as_str()
        .unwrap()
        .is_empty());
    assert!(
        value["recent_invocations"][0]["raw"]["tokens"]
            .as_u64()
            .unwrap()
            > value["recent_invocations"][0]["emitted"]["tokens"]
                .as_u64()
                .unwrap()
    );
    assert_eq!(value["recent_invocations"][0]["source"], "grep");
    assert_eq!(value["daily_totals"].as_array().unwrap().len(), 1);
    assert_eq!(value["recent_failure_artifacts"][0]["tool_name"], "grep");
    assert_eq!(value["recent_failure_artifacts"][0]["original_bytes"], 16);
}

fn assert_export_json_presentation_sections(value: &serde_json::Value) {
    assert_eq!(value["presentation"]["metrics"]["invocations"], 2);
    assert_eq!(value["presentation"]["metrics"]["failures"], 1);
    assert_eq!(value["presentation"]["metrics"]["expansions"], 0);
    assert_eq!(value["presentation"]["metrics"]["failure_rate"], 0.5);
    assert!(
        value["presentation"]["metrics"]["raw"]["tokens"]
            .as_u64()
            .unwrap()
            > value["presentation"]["metrics"]["emitted"]["tokens"]
                .as_u64()
                .unwrap()
    );
    assert!(
        value["presentation"]["metrics"]["saved"]["tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(value["presentation"]["metrics"]["expanded"]["tokens"], 0);
    assert_eq!(
        value["presentation"]["metrics"]["savings_distribution"]["saving_invocations"],
        2
    );
    assert!(
        value["presentation"]["metrics"]["net_token_delta"]
            .as_i64()
            .unwrap()
            < 0
    );
    assert_eq!(
        value["presentation"]["metrics"]["context_window_tokens"],
        DEFAULT_PRESENTATION_CONTEXT_WINDOW_TOKENS
    );
    assert!(value["presentation"]["slide_outline"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().unwrap().contains("Measured result")));
    assert!(value["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["title"].as_str().unwrap().contains("Protect")));
    assert_eq!(value["operational_health"]["missing_artifact_risks"], 0);
    assert_eq!(value["failure_focus"][0]["command"], "grep");
    assert_eq!(value["failure_focus"][0]["artifact_count"], 1);
}

fn assert_export_json_report_sections(value: &serde_json::Value) {
    assert_eq!(value["command_report_totals"][0]["command"], "grep");
    assert_eq!(value["command_report_totals"][0]["reports"], 1);
    assert_eq!(value["recent_command_reports"][0]["command_root"], "grep");
    assert_eq!(
        value["recent_command_reports"][0]["issue_kind"],
        "suspicious_output"
    );
    assert_eq!(
        value["recent_command_reports"][0]["evidence_kind"],
        "no-match"
    );
    assert_eq!(
        value["recent_command_reports"][0]["cx_response_recorded"],
        false
    );
}

fn assert_export_json_opportunity_sections(value: &serde_json::Value) {
    assert_eq!(
        value["passthrough_opportunities"][0]["process"],
        "passthrough"
    );
    assert_eq!(
        value["passthrough_opportunities"][0]["command_family"],
        "passthrough seq"
    );
    assert_eq!(
        value["passthrough_opportunities"][0]["strategy"],
        "test-generic"
    );
    assert!(
        value["passthrough_opportunities"][0]["potential_saved"]["tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
}

fn assert_export_json_routing_sections(value: &serde_json::Value) {
    assert_eq!(value["routing_summary"]["rejections"], 1);
    assert_eq!(value["routing_summary"]["passthrough_disabled"], 1);
    assert_eq!(value["routing_decision_totals"][0]["command_root"], "git");
    assert_eq!(
        value["routing_decision_totals"][0]["command_family"],
        "git branch"
    );
    assert_eq!(
        value["recent_routing_decisions"][0]["command_family"],
        "git branch"
    );
    assert_eq!(
        value["recent_routing_decisions"][0]["reason"],
        "passthrough-disabled"
    );
    assert_eq!(value["operational_health"]["routing_rejections"], 1);
}

#[test]
fn run_export_csv_keeps_report_evidence_when_invocation_snapshot_is_empty() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            record_command_report(&CommandReportRecord {
                command: "cx git diff -- src",
                command_family: "git diff",
                command_shape: "",
                command_shape_hash: "",
                issue_kind: "incorrect_summary",
                note: "test report",
            })
            .unwrap();

            let output = run_export(ExportFormat::Csv, 3, CommandFilter::default()).unwrap();
            assert!(output.stdout.contains("overall,,no_data,true"));
            assert!(output
                .stdout
                .contains("command_report_totals,1,reports,1,git,,git diff"));
            assert!(output.stdout.contains(
                "recent_command_reports,1,issue_kind,incorrect_summary,git,git diff,git diff"
            ));
        },
    );
}

#[test]
fn run_export_keeps_routing_evidence_when_invocation_snapshot_is_empty() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    crate::support::test_support::with_env_vars(
        &[
            (
                "CX_INSIGHTS_DB_PATH",
                Some(db_path.to_string_lossy().as_ref()),
            ),
            ("CX_DISABLE_INSIGHTS", None),
        ],
        || {
            record_routing_rejection(&RoutingDecisionRecord {
                args: &["cx".into(), "--".into(), "read".into(), "--head".into()],
                reason: "cx-owned-parse-error",
                error_kind: "missing-required-argument",
                explicit_auto: true,
                passthrough_eligible: false,
                passthrough_enabled: true,
            })
            .unwrap();

            let json = run_export(ExportFormat::Json, 3, CommandFilter::default()).unwrap();
            let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
            assert_eq!(value["no_data"], true);
            assert_eq!(value["routing_summary"]["rejections"], 1);
            assert_eq!(value["recent_routing_decisions"][0]["process"], "read");

            let csv = run_export(ExportFormat::Csv, 3, CommandFilter::default()).unwrap();
            assert!(csv.stdout.contains("overall,,no_data,true"));
            assert!(csv.stdout.contains("routing_summary,,rejections,1"));
            assert!(csv
                .stdout
                .contains("recent_routing_decisions,1,reason,cx-owned-parse-error"));
        },
    );
}
