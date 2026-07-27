use anyhow::Result;

use crate::support::runner::{append_failure_hint, capture, ProxyOutcome};
use crate::support::utils::{fallback_window, resolved_command};

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    let show_all = args.iter().any(|arg| {
        (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('a')) || arg == "--all"
    });

    let flags: Vec<&str> = args
        .iter()
        .filter(|arg| arg.starts_with('-'))
        .map(String::as_str)
        .collect();
    let paths: Vec<&str> = args
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect();

    let mut cmd = resolved_command("ls");
    cmd.arg("-la");
    for flag in flags {
        if flag.starts_with("--") {
            if flag != "--all" {
                cmd.arg(flag);
            }
        } else {
            let extra: String = flag
                .trim_start_matches('-')
                .chars()
                .filter(|ch| *ch != 'l' && *ch != 'a' && *ch != 'h')
                .collect();
            if !extra.is_empty() {
                cmd.arg(format!("-{extra}"));
            }
        }
    }

    if paths.is_empty() {
        cmd.arg(".");
    } else {
        for path in paths {
            cmd.arg(path);
        }
    }

    let mut output = capture(cmd, "ls")?;
    let exit_code = output.exit_code;

    if exit_code != 0 {
        let failure_hint = output.failure_artifact_hint("ls");
        let observation = output.observation("ls");
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(
                if output.stdout.trim().is_empty() {
                    String::new()
                } else {
                    fallback_window(&output.stdout, 12, 28)
                },
                failure_hint.as_deref(),
            ),
            stderr: output.stderr.trim_end().to_string(),
            exit_code,
            observation: None,
        }
        .with_observation(observation));
    }

    let (entries, summary) = compact_ls(&output.stdout, show_all);
    let stdout = if summary.is_empty() {
        entries
    } else {
        format!("{entries}{summary}")
    };

    Ok(ProxyOutcome {
        stdout,
        stderr: String::new(),
        exit_code,
        observation: None,
    }
    .with_observation(output.observation("ls"))
    .with_expansion_reason("directory-summary"))
}

fn compact_ls(raw: &str, show_all: bool) -> (String, String) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut by_ext = std::collections::BTreeMap::<String, usize>::new();

    for line in raw.lines() {
        if line.starts_with("total ") || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }
        let name = parts[8..].join(" ");
        if name == "." || name == ".." {
            continue;
        }
        if !show_all
            && matches!(
                name.as_str(),
                ".git" | "node_modules" | "target" | "__pycache__"
            )
        {
            continue;
        }
        let is_dir = parts[0].starts_with('d');
        if is_dir {
            dirs.push(name);
        } else {
            let size = human_size(parts[4].parse::<u64>().unwrap_or(0));
            let ext = name
                .rsplit('.')
                .next()
                .filter(|ext| *ext != name)
                .map(|ext| format!(".{ext}"))
                .unwrap_or_else(|| "no ext".to_string());
            *by_ext.entry(ext).or_insert(0) += 1;
            files.push((name, size));
        }
    }

    if dirs.is_empty() && files.is_empty() {
        return ("(empty)\n".to_string(), String::new());
    }

    let mut entries = String::new();
    for dir in &dirs {
        entries.push_str(dir);
        entries.push_str("/\n");
    }
    for (name, size) in &files {
        entries.push_str(&format!("{name}  {size}\n"));
    }

    let mut summary = format!("\nSummary: {} files, {} dirs", files.len(), dirs.len());
    if !by_ext.is_empty() {
        let ext_summary = by_ext
            .iter()
            .rev()
            .take(5)
            .map(|(ext, count)| format!("{count} {ext}"))
            .collect::<Vec<_>>()
            .join(", ");
        summary.push_str(&format!(" ({ext_summary})"));
    }
    summary.push('\n');
    (entries, summary)
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    #[test]
    fn compact_ls_filters_noise_dirs() {
        let raw = "total 8\n\
                   drwxr-xr-x  2 user  staff   64 Jan 1 12:00 node_modules\n\
                   drwxr-xr-x  2 user  staff   64 Jan 1 12:00 src\n\
                   -rw-r--r--  1 user  staff 1234 Jan 1 12:00 Cargo.toml\n";
        let (entries, summary) = compact_ls(raw, false);
        assert!(entries.contains("src/"));
        assert!(entries.contains("Cargo.toml"));
        assert!(!entries.contains("node_modules"));
        assert!(summary.contains("1 files, 1 dirs"));
    }

    #[cfg(unix)]
    #[test]
    fn run_reads_fake_ls_from_path() {
        crate::support::test_support::with_fake_path(
            &[(
                "ls",
                "#!/bin/sh\ncat <<'EOF'\n\
drwxr-xr-x  2 user  staff   64 Jan 1 12:00 src\n\
-rw-r--r--  1 user  staff 1234 Jan 1 12:00 Cargo.toml\n\
EOF\n",
            )],
            || {
                let output = run(&[]).unwrap();
                assert!(output.stdout.contains("src/"));
                assert!(output.stdout.contains("Cargo.toml"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_stores_failure_artifact() {
        let temp = tempdir().unwrap();
        let bin = temp.path().join("bin");
        let home = temp.path().join("home");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        crate::support::test_support::write_executable(
            &bin,
            "ls",
            "#!/bin/sh\nprintf 'ls: missing path\\n' >&2\nexit 2\n",
        );

        crate::support::test_support::with_env_vars(
            &[
                ("PATH", Some(bin.to_string_lossy().as_ref())),
                ("HOME", Some(home.to_string_lossy().as_ref())),
                ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
                ("CX_TOOL_FALLBACK_PATHS", None),
            ],
            || {
                let output = run(&["missing".to_string()]).unwrap();
                assert_eq!(output.exit_code, 2);
                assert!(output.stderr.contains("missing path"));
                assert!(output
                    .stdout
                    .contains("[full output: ~/.cx/cache/failures/ls/"));
                let artifact_dir = home.join(".cx/cache/failures/ls");
                assert_eq!(fs::read_dir(artifact_dir).unwrap().count(), 1);
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_returns_without_waiting_for_descendant_stdout_to_close() {
        crate::support::test_support::with_fake_path(
            &[(
                "ls",
                "#!/bin/sh\n(sleep 1) &\ncat <<'EOF'\n\
drwxr-xr-x  2 user  staff   64 Jan 1 12:00 src\n\
-rw-r--r--  1 user  staff 1234 Jan 1 12:00 Cargo.toml\n\
EOF\n",
            )],
            || {
                let start = Instant::now();
                let output = run(&[]).unwrap();
                assert!(output.stdout.contains("src/"));
                assert!(output.stdout.contains("Cargo.toml"));
                assert!(start.elapsed() < Duration::from_millis(700));
            },
        );
    }
}
