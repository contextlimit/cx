use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let git_path = dir.join(".git");
        if git_path.exists() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

pub fn global_cache_root() -> Result<PathBuf> {
    Ok(global_state_root()?.join("cache"))
}

pub fn global_state_root() -> Result<PathBuf> {
    Ok(home_dir()?.join(".cx"))
}

pub fn global_db_file() -> Result<PathBuf> {
    Ok(global_state_root()?.join("db.sqlite"))
}

pub fn global_config_root() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("cx"));
    }
    Ok(home_dir()?.join(".config").join("cx"))
}

pub fn user_config_file() -> Result<PathBuf> {
    Ok(global_config_root()?.join("config.toml"))
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_git_root_from_nested_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src/lib")).unwrap();
        let found = find_git_root(&root.join("src/lib")).unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn global_cache_root_uses_home_cx_directory() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();

        crate::support::test_support::with_env_vars(
            &[("HOME", Some(home.to_string_lossy().as_ref()))],
            || {
                let root = global_cache_root().unwrap();
                assert_eq!(root, home.join(".cx/cache"));
            },
        );
    }

    #[test]
    fn global_db_file_uses_home_cx_database() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();

        crate::support::test_support::with_env_vars(
            &[("HOME", Some(home.to_string_lossy().as_ref()))],
            || {
                let path = global_db_file().unwrap();
                assert_eq!(path, home.join(".cx/db.sqlite"));
            },
        );
    }
}
