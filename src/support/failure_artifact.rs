use std::cmp::Reverse;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::support::paths::global_cache_root;

const MAX_FAILURE_ARTIFACTS_PER_TOOL: usize = 20;
const DEFAULT_MAX_FAILURE_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_MAX_FAILURE_ARTIFACT_AGE_DAYS: u64 = 30;
const DEFAULT_COMPRESSION_THRESHOLD_BYTES: u64 = 1024 * 1024;
static ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureArtifact {
    pub path: PathBuf,
    pub display_path: String,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub original_bytes: u64,
    pub stored_bytes: u64,
    pub compression: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureArtifactSummary {
    pub tool_name: String,
    pub display_dir: String,
    pub count: usize,
    pub latest_display_path: Option<String>,
}

pub fn store_failure_artifact(
    tool_name: &str,
    stdout: &str,
    stderr: &str,
) -> Result<FailureArtifact> {
    let cwd = env::current_dir().context("failed to resolve current directory for tee artifact")?;
    store_failure_artifact_for_cwd(tool_name, stdout, stderr, &cwd)
}

pub fn store_failure_artifact_for_cwd(
    tool_name: &str,
    stdout: &str,
    stderr: &str,
    cwd: &Path,
) -> Result<FailureArtifact> {
    store_failure_artifact_with(
        tool_name,
        stdout.len() as u64,
        stderr.len() as u64,
        cwd,
        |writer| {
            write_artifact_body(
                writer,
                &mut io::Cursor::new(stdout.as_bytes()),
                &mut io::Cursor::new(stderr.as_bytes()),
            )
        },
    )
}

pub fn store_failure_artifact_from_files(
    tool_name: &str,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<FailureArtifact> {
    let cwd = env::current_dir().context("failed to resolve current directory for tee artifact")?;
    store_failure_artifact_from_files_for_cwd(tool_name, stdout_path, stderr_path, &cwd)
}

pub fn store_failure_artifact_from_files_for_cwd(
    tool_name: &str,
    stdout_path: &Path,
    stderr_path: &Path,
    cwd: &Path,
) -> Result<FailureArtifact> {
    let stdout_bytes = file_len(stdout_path)?;
    let stderr_bytes = file_len(stderr_path)?;
    store_failure_artifact_with(tool_name, stdout_bytes, stderr_bytes, cwd, |writer| {
        let mut stdout = File::open(stdout_path)
            .with_context(|| format!("failed to open {}", stdout_path.display()))?;
        let mut stderr = File::open(stderr_path)
            .with_context(|| format!("failed to open {}", stderr_path.display()))?;
        write_artifact_body(writer, &mut stdout, &mut stderr)
    })
}

fn store_failure_artifact_with<F>(
    tool_name: &str,
    stdout_bytes: u64,
    stderr_bytes: u64,
    cwd: &Path,
    write_body: F,
) -> Result<FailureArtifact>
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    let root = resolve_failure_root(cwd)?;
    let tool_dir = root.dir.join(tool_name);
    fs::create_dir_all(&tool_dir)
        .with_context(|| format!("failed to create {}", tool_dir.display()))?;

    let original_bytes = stdout_bytes.saturating_add(stderr_bytes);
    let compressed = original_bytes >= compression_threshold_bytes();
    let extension = if compressed { "log.gz" } else { "log" };
    let sequence = ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let filename = format!("{}-{}-{sequence}.{extension}", now_ms(), std::process::id());
    let path = tool_dir.join(filename);
    write_failure_artifact_atomic(&path, compressed, write_body)?;
    let stored_bytes = file_len(&path)?;
    prune_old_artifacts(&tool_dir)?;
    prune_global_artifacts(&root.dir)?;

    let display_path =
        format_artifact_display_path(&root.display_prefix, tool_name, path.file_name());

    let artifact = FailureArtifact {
        path,
        display_path,
        stdout_bytes,
        stderr_bytes,
        original_bytes,
        stored_bytes,
        compression: if compressed { "gzip" } else { "none" }.to_string(),
    };
    let record = crate::support::insights::FailureArtifactRecord {
        display_path: &artifact.display_path,
        tool_name,
        compression: &artifact.compression,
        stdout_bytes: artifact.stdout_bytes,
        stderr_bytes: artifact.stderr_bytes,
        original_bytes: artifact.original_bytes,
        stored_bytes: artifact.stored_bytes,
    };
    if let Err(error) = crate::support::insights::record_failure_artifact(&record) {
        if env::var("CX_INSIGHTS_DEBUG").is_ok() {
            eprintln!("cx failure artifact insights: {error:#}");
        }
    }
    Ok(artifact)
}

pub fn failure_artifact_summary(tool_name: &str) -> Result<FailureArtifactSummary> {
    let cwd =
        env::current_dir().context("failed to resolve current directory for failure artifacts")?;
    let root = resolve_failure_root(&cwd)?;
    let tool_dir = root.dir.join(tool_name);
    let display_dir = format_artifact_display_path(&root.display_prefix, tool_name, None);
    if !tool_dir.is_dir() {
        return Ok(FailureArtifactSummary {
            tool_name: tool_name.to_string(),
            display_dir,
            count: 0,
            latest_display_path: None,
        });
    }

    let mut entries = fs::read_dir(&tool_dir)
        .with_context(|| format!("failed to read {}", tool_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| is_artifact_file(&entry.path()))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| Reverse(entry.file_name()));

    let latest_display_path = entries.first().map(|entry| {
        let file_name = entry.file_name();
        format_artifact_display_path(&root.display_prefix, tool_name, Some(&file_name))
    });

    Ok(FailureArtifactSummary {
        tool_name: tool_name.to_string(),
        display_dir,
        count: entries.len(),
        latest_display_path,
    })
}

struct FailureRoot {
    dir: PathBuf,
    display_prefix: String,
}

fn resolve_failure_root(cwd: &Path) -> Result<FailureRoot> {
    let _ = cwd;
    let dir = global_cache_root()?.join("failures");
    Ok(FailureRoot {
        dir,
        display_prefix: display_global_prefix()?,
    })
}

fn display_global_prefix() -> Result<String> {
    env::var_os("HOME")
        .map(|_| "~/.cx/cache/failures".to_string())
        .context("HOME is not set; cannot resolve global tee artifact path")
}

fn format_artifact_display_path(
    display_prefix: &str,
    tool_name: &str,
    file_name: Option<&std::ffi::OsStr>,
) -> String {
    let display_dir = if display_prefix.is_empty() {
        tool_name.to_string()
    } else {
        format!("{display_prefix}/{tool_name}")
    };
    file_name.map_or(display_dir.clone(), |name| {
        format!("{}/{}", display_dir, name.to_string_lossy())
    })
}

fn write_failure_artifact_atomic<F>(path: &Path, compressed: bool, write_body: F) -> Result<()>
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    let temp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("artifact")
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .with_context(|| format!("failed to create {}", temp_path.display()))?;

    let write_result = if compressed {
        let mut encoder = GzEncoder::new(file, Compression::fast());
        write_body(&mut encoder).and_then(|()| {
            let file = encoder
                .finish()
                .context("failed to finish gzip failure artifact")?;
            file.sync_all()
                .context("failed to sync gzip failure artifact")
        })
    } else {
        let mut file = file;
        write_body(&mut file).and_then(|()| {
            file.flush().context("failed to flush failure artifact")?;
            file.sync_all().context("failed to sync failure artifact")
        })
    };
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(())
}

fn write_artifact_body(
    writer: &mut dyn Write,
    stdout: &mut dyn Read,
    stderr: &mut dyn Read,
) -> Result<()> {
    writer.write_all(b"[stdout]\n")?;
    io::copy(stdout, writer)?;
    writer.write_all(b"\n\n[stderr]\n")?;
    io::copy(stderr, writer)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn file_len(path: &Path) -> Result<u64> {
    fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))
        .map(|metadata| metadata.len())
}

fn compression_threshold_bytes() -> u64 {
    env_u64(
        "CX_FAILURE_ARTIFACT_COMPRESSION_THRESHOLD_BYTES",
        DEFAULT_COMPRESSION_THRESHOLD_BYTES,
    )
}

fn max_failure_artifact_bytes() -> u64 {
    env_u64(
        "CX_FAILURE_ARTIFACT_MAX_TOTAL_BYTES",
        DEFAULT_MAX_FAILURE_ARTIFACT_BYTES,
    )
}

fn max_failure_artifact_age() -> Duration {
    Duration::from_secs(
        env_u64(
            "CX_FAILURE_ARTIFACT_MAX_AGE_DAYS",
            DEFAULT_MAX_FAILURE_ARTIFACT_AGE_DAYS,
        )
        .saturating_mul(24 * 60 * 60),
    )
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

fn is_artifact_file(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".log") || name.ends_with(".log.gz"))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn prune_old_artifacts(root: &Path) -> Result<()> {
    let mut entries = fs::read_dir(root)
        .with_context(|| format!("failed to read {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| is_artifact_file(&entry.path()))
        .collect::<Vec<_>>();

    entries.sort_by_key(|entry| Reverse(entry.file_name()));
    for stale in entries.into_iter().skip(MAX_FAILURE_ARTIFACTS_PER_TOOL) {
        let _ = fs::remove_file(stale.path());
    }
    Ok(())
}

fn prune_global_artifacts(root: &Path) -> Result<()> {
    let mut entries = Vec::new();
    if !root.is_dir() {
        return Ok(());
    }
    for tool in fs::read_dir(root)
        .with_context(|| format!("failed to read {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
    {
        for entry in fs::read_dir(tool.path())
            .with_context(|| format!("failed to read {}", tool.path().display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| is_artifact_file(&entry.path()))
        {
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            entries.push(ArtifactEntry {
                path: entry.path(),
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            });
        }
    }
    entries.sort_by_key(|entry| Reverse((entry.modified, entry.path.clone())));

    let now = SystemTime::now();
    let max_age = max_failure_artifact_age();
    let mut retained = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let expired = now
            .duration_since(entry.modified)
            .is_ok_and(|age| age > max_age);
        if index > 0 && expired {
            let _ = fs::remove_file(entry.path);
        } else {
            retained.push(entry);
        }
    }

    let budget = max_failure_artifact_bytes();
    let mut retained_bytes = 0u64;
    for (index, entry) in retained.into_iter().enumerate() {
        if index > 0 && retained_bytes.saturating_add(entry.bytes) > budget {
            let _ = fs::remove_file(entry.path);
        } else {
            retained_bytes = retained_bytes.saturating_add(entry.bytes);
        }
    }
    Ok(())
}

struct ArtifactEntry {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use tempfile::tempdir;

    #[test]
    fn stores_failure_artifact_under_home_even_in_git_repo() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("project");
        let home = temp.path().join("home");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&home).unwrap();

        crate::support::test_support::with_env_vars(
            &[("HOME", Some(home.to_string_lossy().as_ref()))],
            || {
                let artifact =
                    store_failure_artifact_for_cwd("pytest", "hello", "boom", &root).unwrap();
                assert!(artifact.path.exists());
                assert!(artifact
                    .path
                    .starts_with(home.join(".cx/cache/failures/pytest")));
                assert!(artifact
                    .display_path
                    .starts_with("~/.cx/cache/failures/pytest/"));
                let body = fs::read_to_string(&artifact.path).unwrap();
                assert!(body.contains("[stdout]"));
                assert!(body.contains("[stderr]"));
                assert!(!root.join(".cx").exists());
            },
        );
    }

    #[test]
    fn prunes_old_artifacts() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("project");
        let dir = root.join(".cx/cache/failures");
        fs::create_dir_all(&dir).unwrap();
        for index in 0..25 {
            let path = dir.join(format!("{index:04}.log"));
            fs::write(path, "x").unwrap();
        }

        prune_old_artifacts(&dir).unwrap();
        let count = fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, MAX_FAILURE_ARTIFACTS_PER_TOOL);
    }

    #[test]
    fn stores_global_failure_artifact_under_home_cx_cache() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();

        crate::support::test_support::with_env_vars(
            &[("HOME", Some(home.to_string_lossy().as_ref()))],
            || {
                let cwd = temp.path().join("outside");
                fs::create_dir_all(&cwd).unwrap();
                let artifact =
                    store_failure_artifact_for_cwd("git", "hello", "boom", &cwd).unwrap();
                assert!(artifact
                    .path
                    .starts_with(home.join(".cx/cache/failures/git")));
                assert!(artifact
                    .display_path
                    .starts_with("~/.cx/cache/failures/git/"));
            },
        );
    }

    #[test]
    fn streams_large_failure_artifacts_into_gzip() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();

        crate::support::test_support::with_env_vars(
            &[
                ("HOME", Some(home.to_string_lossy().as_ref())),
                (
                    "CX_FAILURE_ARTIFACT_COMPRESSION_THRESHOLD_BYTES",
                    Some("32"),
                ),
            ],
            || {
                let stdout = "repeated failure evidence\n".repeat(512);
                let artifact =
                    store_failure_artifact_for_cwd("grep", &stdout, "boom", temp.path()).unwrap();
                assert_eq!(artifact.compression, "gzip");
                assert!(artifact.path.to_string_lossy().ends_with(".log.gz"));
                assert!(artifact.stored_bytes < artifact.original_bytes);

                let mut body = String::new();
                GzDecoder::new(File::open(&artifact.path).unwrap())
                    .read_to_string(&mut body)
                    .unwrap();
                assert!(body.contains("[stdout]"));
                assert!(body.contains("repeated failure evidence"));
                assert!(body.contains("[stderr]\nboom"));
            },
        );
    }

    #[test]
    fn global_byte_budget_keeps_newest_recovery_artifact() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();

        crate::support::test_support::with_env_vars(
            &[
                ("HOME", Some(home.to_string_lossy().as_ref())),
                (
                    "CX_FAILURE_ARTIFACT_COMPRESSION_THRESHOLD_BYTES",
                    Some("1000000"),
                ),
                ("CX_FAILURE_ARTIFACT_MAX_TOTAL_BYTES", Some("160")),
            ],
            || {
                for index in 0..4 {
                    store_failure_artifact_for_cwd(
                        "node",
                        &format!("artifact-{index}-{}", "x".repeat(80)),
                        "failure",
                        temp.path(),
                    )
                    .unwrap();
                }
                let dir = home.join(".cx/cache/failures/node");
                let entries = fs::read_dir(&dir)
                    .unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| is_artifact_file(&entry.path()))
                    .collect::<Vec<_>>();
                assert_eq!(entries.len(), 1);
                let body = fs::read_to_string(entries[0].path()).unwrap();
                assert!(body.contains("artifact-3"));
            },
        );
    }

    #[test]
    fn summarizes_latest_failure_artifact_for_tool() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        let artifact_dir = home.join(".cx/cache/failures/grep");
        fs::create_dir_all(&artifact_dir).unwrap();
        fs::write(artifact_dir.join("001.log"), "older").unwrap();
        fs::write(artifact_dir.join("002.log"), "newer").unwrap();

        crate::support::test_support::with_env_vars(
            &[("HOME", Some(home.to_string_lossy().as_ref()))],
            || {
                let summary = failure_artifact_summary("grep").unwrap();
                assert_eq!(summary.tool_name, "grep");
                assert_eq!(summary.display_dir, "~/.cx/cache/failures/grep");
                assert_eq!(summary.count, 2);
                assert_eq!(
                    summary.latest_display_path.as_deref(),
                    Some("~/.cx/cache/failures/grep/002.log")
                );
            },
        );
    }
}
