use std::env;
use std::ffi::OsString;

use cx::commands::insights::run_dashboard;
use cx::support::insights::{
    record_command_opportunity, record_command_report, record_invocation, record_routing_rejection,
    set_insight_setting, update_command_report_disposition, CommandFilter,
    CommandOpportunityRecord, CommandReportDispositionRecord, CommandReportRecord,
    CommandReportStatus, InvocationRecord, OpportunityConfidence, OutputObservation,
    RoutingDecisionRecord, TextMetrics,
};

#[test]
fn dashboard_contract_is_bounded_versioned_and_ui_complete() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite");
    let home = temp.path().join("home");
    let _env = EnvGuard::set(&[
        ("HOME", Some(home.to_string_lossy().as_ref())),
        (
            "CX_INSIGHTS_DB_PATH",
            Some(db_path.to_string_lossy().as_ref()),
        ),
        ("CX_DISABLE_INSIGHTS", None),
    ]);

    seed_dashboard_contract_fixture();
    let output = run_dashboard(3, CommandFilter::default()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_dashboard_contract(&value);
}

fn seed_dashboard_contract_fixture() {
    set_insight_setting("record_invocations", "true").unwrap();
    set_insight_setting("record_command_text", "false").unwrap();
    for index in 0..12 {
        let command = if index % 2 == 0 { "git diff" } else { "grep" };
        let raw = OutputObservation::from_text(command, &"evidence\n".repeat(30 + index));
        record_invocation(&InvocationRecord {
            command,
            exit_code: i32::from(index == 11),
            raw: Some(&raw),
            emitted: TextMetrics::from_text("evidence\n"),
        })
        .unwrap();
    }
    let report = record_command_report(&CommandReportRecord {
        command: "cx grep broad-pattern src",
        command_family: "grep",
        command_shape: "",
        command_shape_hash: "",
        issue_kind: "suspicious_output",
        note: "fixture quality report",
    })
    .unwrap();
    update_command_report_disposition(&CommandReportDispositionRecord {
        report_id: report.id,
        status: CommandReportStatus::Resolved,
        denial_reason: None,
        related_report_id: None,
        note: "covered by a focused regression",
        revision: "r111",
    })
    .unwrap();
    record_command_opportunity(&CommandOpportunityRecord {
        process: "passthrough",
        command_family: "passthrough jq",
        command: "jq fixture",
        source: "jq",
        strategy: "bounded-head-tail",
        confidence: OpportunityConfidence::Low,
        raw: TextMetrics::from_text(&"json-row\n".repeat(200)),
        projected: TextMetrics::from_text("json-row\n... [180 lines omitted] ...\njson-row\n"),
    })
    .unwrap();
    seed_routing_rejections();
}

fn assert_dashboard_contract(value: &serde_json::Value) {
    assert_dashboard_metadata(value);
    assert_dashboard_tables(value);
    assert_dashboard_report_lifecycle(value);
    assert_eq!(
        value["tables"]["recent_quality_reports"][0]["evidence_kind"],
        "no-match"
    );
    assert_eq!(
        value["tables"]["recent_quality_reports"][0]["cx_response_recorded"],
        false
    );
    assert_eq!(value["health"]["routing_summary"]["rejections"], 12);
    assert!(
        value["tables"]["recent_routing_decisions"]
            .as_array()
            .unwrap()
            .len()
            <= 3
    );
    assert!(
        value["presentation"]["metrics"]["saved"]["tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
}

fn assert_dashboard_metadata(value: &serde_json::Value) {
    assert_eq!(value["schema_name"], "cx-insights-dashboard");
    assert_eq!(value["schema_version"], 12);
    assert_eq!(value["source_export_schema_version"], 18);
    assert_eq!(value["limit"], 3);
    assert_eq!(value["summary"]["invocations"], 12);
    assert_eq!(value["savings_distribution"]["invocations"], 12);
    assert_eq!(value["savings_distribution"]["saving_invocations"], 12);
    assert!(
        value["savings_distribution"]["concentration"]["saved_tokens_excluding_top_10"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        value["contract"]["distribution_semantics"]["independent_of_row_limit"],
        true
    );
    assert_eq!(value["settings"]["values"]["record_command_text"], false);
    assert_eq!(
        value["capabilities"]["command_text_recording_enabled"],
        false
    );
    assert_eq!(
        value["empty_state"]["command_text_recording_disabled"],
        true
    );
}

fn assert_dashboard_tables(value: &serde_json::Value) {
    assert!(
        value["tables"]["recent_invocations"]
            .as_array()
            .unwrap()
            .len()
            <= 3
    );
    assert!(
        value["tables"]["largest_invocations"]
            .as_array()
            .unwrap()
            .len()
            <= 3
    );
    assert!(
        value["tables"]["command_families"]
            .as_array()
            .unwrap()
            .len()
            <= 3
    );
    assert!(value["tables"]["recent_invocations"][0]["raw"]["tokens"].is_u64());
    assert!(value["tables"]["recent_invocations"][0]["emitted"]["tokens"].is_u64());
    assert_eq!(
        value["tables"]["passthrough_opportunities"][0]["estimate"],
        true
    );
}

fn assert_dashboard_report_lifecycle(value: &serde_json::Value) {
    assert_eq!(value["health"]["quality_reports"], 1);
    assert_eq!(value["health"]["open_quality_reports"], 0);
    assert_eq!(value["health"]["closed_quality_reports"], 1);
    assert_eq!(value["quality_report_status"]["resolved"], 1);
    assert_eq!(value["tables"]["quality_report_families"][0]["open"], 0);
    assert_eq!(value["tables"]["quality_report_families"][0]["resolved"], 1);
    assert_eq!(
        value["tables"]["recent_quality_reports"][0]["status"],
        "resolved"
    );
    assert_eq!(
        value["tables"]["recent_quality_reports"][0]["resolution_revision"],
        "r111"
    );
}

fn seed_routing_rejections() {
    for index in 0..12 {
        record_routing_rejection(&RoutingDecisionRecord {
            args: &[
                "cx".into(),
                "--".into(),
                "git".into(),
                "branch".into(),
                format!("shape-{index}"),
            ],
            reason: "passthrough-disabled",
            error_kind: "invalid-subcommand",
            explicit_auto: true,
            passthrough_eligible: true,
            passthrough_enabled: false,
        })
        .unwrap();
    }
}

struct EnvGuard {
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    fn set(entries: &[(&'static str, Option<&str>)]) -> Self {
        let previous = entries
            .iter()
            .map(|(key, _)| (*key, env::var_os(key)))
            .collect();
        for (key, value) in entries {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
        Self { previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }
}
