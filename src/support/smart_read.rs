use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::support::config::load_user_config;
use crate::support::runner::capture_with_stdin_timeout;

const DEFAULT_PLUGIN_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_SMART_MAX_LINES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartReadOutput {
    pub summary: Option<String>,
    pub stderr_note: Option<String>,
}

#[derive(Debug, Serialize)]
struct SmartReadRequest<'a> {
    file: String,
    cwd: String,
    language: &'a str,
    content: &'a str,
    max_lines: usize,
    mode: &'static str,
}

#[derive(Debug, Deserialize)]
struct SmartReadResponse {
    summary: String,
}

pub fn summarize(
    path: &Path,
    content: &str,
    language: &str,
    max_lines: Option<usize>,
) -> SmartReadOutput {
    let plugin = match resolve_plugin() {
        Ok(plugin) => plugin,
        Err(error) => {
            return SmartReadOutput {
                summary: None,
                stderr_note: Some(format!("cx smart-read plugin disabled: {error}")),
            };
        }
    };
    let Some(plugin) = plugin else {
        return SmartReadOutput {
            summary: None,
            stderr_note: None,
        };
    };

    match run_plugin(
        &plugin.command,
        plugin.timeout_ms,
        path,
        content,
        language,
        max_lines,
    ) {
        Ok(summary) => SmartReadOutput {
            summary: Some(summary),
            stderr_note: None,
        },
        Err(error) => SmartReadOutput {
            summary: None,
            stderr_note: Some(format!("cx smart-read plugin failed: {error}")),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginConfig {
    command: String,
    timeout_ms: u64,
}

fn resolve_plugin() -> Result<Option<PluginConfig>> {
    if let Ok(command) = std::env::var("CX_SMART_READ_COMMAND") {
        let trimmed = command.trim();
        if !trimmed.is_empty() {
            return Ok(Some(PluginConfig {
                command: trimmed.to_string(),
                timeout_ms: DEFAULT_PLUGIN_TIMEOUT_MS,
            }));
        }
    }

    let config = load_user_config()?;
    let Some(command) = config.smart_read.command else {
        return Ok(None);
    };
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(PluginConfig {
        command: trimmed.to_string(),
        timeout_ms: config
            .smart_read
            .timeout_ms
            .unwrap_or(DEFAULT_PLUGIN_TIMEOUT_MS),
    }))
}

fn run_plugin(
    command: &str,
    timeout_ms: u64,
    path: &Path,
    content: &str,
    language: &str,
    max_lines: Option<usize>,
) -> Result<String> {
    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    let request = SmartReadRequest {
        file: path.display().to_string(),
        cwd: cwd.display().to_string(),
        language,
        content,
        max_lines: max_lines.unwrap_or(DEFAULT_SMART_MAX_LINES),
        mode: "smart",
    };
    let encoded = serde_json::to_vec(&request).context("failed to encode smart-read request")?;

    let tool_name = format!("smart-read plugin {command}");
    let output = capture_with_stdin_timeout(
        Command::new(command),
        &tool_name,
        encoded,
        Duration::from_millis(timeout_ms),
    )?;

    if output.exit_code != 0 {
        let stderr = output.stderr.trim().to_string();
        if stderr.is_empty() {
            anyhow::bail!("plugin exited with status {}", output.exit_code);
        }
        anyhow::bail!(stderr);
    }

    let response: SmartReadResponse =
        serde_json::from_slice(output.stdout.as_bytes()).context("plugin returned invalid json")?;
    let summary = response.summary.trim();
    if summary.is_empty() {
        anyhow::bail!("plugin returned an empty summary");
    }
    Ok(summary.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::support::test_support::{with_env_vars, write_executable};

    #[test]
    fn env_plugin_wins_over_config() {
        let temp = tempfile::tempdir().unwrap();
        let config_root = temp.path().join("config");
        fs::create_dir_all(config_root.join("cx")).unwrap();
        fs::write(
            config_root.join("cx/config.toml"),
            "[smart_read]\ncommand = \"/tmp/from-config\"\ntimeout_ms = 3200\n",
        )
        .unwrap();

        crate::support::test_support::with_env_vars(
            &[
                (
                    "XDG_CONFIG_HOME",
                    Some(config_root.to_string_lossy().as_ref()),
                ),
                ("CX_SMART_READ_COMMAND", Some("/tmp/from-env")),
            ],
            || {
                let plugin = resolve_plugin().unwrap().unwrap();
                assert_eq!(plugin.command, "/tmp/from-env");
                assert_eq!(plugin.timeout_ms, DEFAULT_PLUGIN_TIMEOUT_MS);
            },
        );
    }

    #[test]
    fn config_plugin_is_used_when_env_is_absent() {
        let temp = tempfile::tempdir().unwrap();
        let config_root = temp.path().join("config");
        fs::create_dir_all(config_root.join("cx")).unwrap();
        fs::write(
            config_root.join("cx/config.toml"),
            "[smart_read]\ncommand = \"/tmp/from-config\"\ntimeout_ms = 3200\n",
        )
        .unwrap();

        with_env_vars(
            &[
                (
                    "XDG_CONFIG_HOME",
                    Some(config_root.to_string_lossy().as_ref()),
                ),
                ("CX_SMART_READ_COMMAND", None),
            ],
            || {
                let plugin = resolve_plugin().unwrap().unwrap();
                assert_eq!(plugin.command, "/tmp/from-config");
                assert_eq!(plugin.timeout_ms, 3200);
            },
        );
    }

    #[test]
    fn summarize_returns_plugin_summary() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = temp.path().join("smart-plugin");
        write_executable(
            temp.path(),
            "smart-plugin",
            "#!/bin/sh\ncat >/dev/null\nprintf '{\"summary\":\"plugin summary\"}'\n",
        );

        with_env_vars(
            &[(
                "CX_SMART_READ_COMMAND",
                Some(plugin.to_string_lossy().as_ref()),
            )],
            || {
                let result = summarize(Path::new("demo.rs"), "pub fn alpha() {}", "rust", None);
                assert_eq!(result.summary.as_deref(), Some("plugin summary"));
                assert!(result.stderr_note.is_none());
            },
        );
    }

    #[test]
    fn summarize_reports_invalid_json_and_falls_back() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = temp.path().join("smart-plugin");
        write_executable(
            temp.path(),
            "smart-plugin",
            "#!/bin/sh\ncat >/dev/null\nprintf 'not-json'\n",
        );

        with_env_vars(
            &[(
                "CX_SMART_READ_COMMAND",
                Some(plugin.to_string_lossy().as_ref()),
            )],
            || {
                let result = summarize(Path::new("demo.rs"), "pub fn alpha() {}", "rust", None);
                assert!(result.summary.is_none());
                assert!(result
                    .stderr_note
                    .as_deref()
                    .is_some_and(|note| note.contains("invalid json")));
            },
        );
    }

    #[test]
    fn run_plugin_times_out() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let plugin = temp.path().join("slow-plugin");
        write_executable(
            temp.path(),
            "slow-plugin",
            "#!/bin/sh\nsleep 0.1\nprintf '{\"summary\":\"late\"}'\n",
        );

        with_env_vars(&[("HOME", Some(home.to_string_lossy().as_ref()))], || {
            let error = run_plugin(
                plugin.to_string_lossy().as_ref(),
                10,
                Path::new("demo.rs"),
                "pub fn alpha() {}",
                "rust",
                None,
            )
            .unwrap_err();
            assert!(error.to_string().contains("timed out"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn run_plugin_returns_without_waiting_for_descendant_stdout_to_close() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let plugin = temp.path().join("descriptor-plugin");
        write_executable(
            temp.path(),
            "descriptor-plugin",
            "#!/bin/sh\n(/bin/sleep 1) &\ncat >/dev/null\nprintf '{\"summary\":\"fast\"}'\n",
        );

        with_env_vars(&[("HOME", Some(home.to_string_lossy().as_ref()))], || {
            let start = Instant::now();
            let summary = run_plugin(
                plugin.to_string_lossy().as_ref(),
                5_000,
                Path::new("demo.rs"),
                "pub fn alpha() {}",
                "rust",
                None,
            )
            .unwrap();

            assert_eq!(summary, "fast");
            assert!(start.elapsed() < Duration::from_millis(700));
        });
    }
}
