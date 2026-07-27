use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::support::paths::user_config_file;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub smart_read: SmartReadConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SmartReadConfig {
    pub command: Option<String>,
    pub timeout_ms: Option<u64>,
}

pub fn load_user_config() -> Result<AppConfig> {
    let path = user_config_file()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn missing_config_returns_default() {
        crate::support::test_support::with_env_vars(
            &[
                ("HOME", Some("/tmp/cx-test-home-missing")),
                ("XDG_CONFIG_HOME", None),
            ],
            || {
                let config = load_user_config().unwrap();
                assert!(config.smart_read.command.is_none());
                assert!(config.smart_read.timeout_ms.is_none());
            },
        );
    }

    #[test]
    fn reads_config_from_xdg_home() {
        let temp = tempfile::tempdir().unwrap();
        let config_root = temp.path().join("config");
        fs::create_dir_all(config_root.join("cx")).unwrap();
        fs::write(
            config_root.join("cx/config.toml"),
            "[smart_read]\ncommand = \"/tmp/helper\"\ntimeout_ms = 4200\n",
        )
        .unwrap();

        crate::support::test_support::with_env_vars(
            &[(
                "XDG_CONFIG_HOME",
                Some(config_root.to_string_lossy().as_ref()),
            )],
            || {
                let config = load_user_config().unwrap();
                assert_eq!(config.smart_read.command.as_deref(), Some("/tmp/helper"));
                assert_eq!(config.smart_read.timeout_ms, Some(4200));
            },
        );
    }
}
