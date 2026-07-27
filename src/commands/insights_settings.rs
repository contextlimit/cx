use anyhow::Result;

use crate::support::insights::{self, InsightSettingsSnapshot};
use crate::support::runner::ProxyOutcome;

pub fn run_settings(assignments: &[String]) -> Result<ProxyOutcome> {
    let parsed = assignments
        .iter()
        .map(|assignment| parse_setting_assignment(assignment))
        .collect::<Result<Vec<_>>>()?;
    let snapshot = if parsed.is_empty() {
        insights::insight_settings_snapshot()?
    } else {
        insights::set_insight_settings(&parsed)?
    };
    Ok(ProxyOutcome::success(format_settings(&snapshot)))
}

fn parse_setting_assignment(assignment: &str) -> Result<(&str, &str)> {
    assignment
        .split_once('=')
        .filter(|(key, value)| !key.trim().is_empty() && !value.trim().is_empty())
        .map(|(key, value)| (key.trim(), value.trim()))
        .ok_or_else(|| {
            anyhow::anyhow!("invalid setting assignment `{assignment}`; use key=true or key=false")
        })
}

fn format_settings(snapshot: &InsightSettingsSnapshot) -> String {
    let mut output = String::from("cx insights: settings\n");
    output.push_str(&format!("Database: {}\n", snapshot.database));
    output.push_str(&format!(
        "Database exists: {}\n",
        if snapshot.database_exists {
            "yes"
        } else {
            "no"
        }
    ));
    output.push_str("setting | value | description\n");
    for row in &snapshot.rows {
        output.push_str(&format!(
            "{} | {} | {}\n",
            row.key, row.value, row.description
        ));
    }
    output.trim_end().to_string()
}
