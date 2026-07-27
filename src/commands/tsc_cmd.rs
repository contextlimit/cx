use std::collections::{BTreeSet, HashMap};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;

use crate::support::runner::{run_filtered, ProxyOutcome, RunOptions};
use crate::support::utils::{resolved_command, tool_exists, truncate};

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolve_tsc_command(args)?;
    for arg in args {
        cmd.arg(arg);
    }

    run_filtered(
        cmd,
        "tsc",
        |output| filter_tsc_output(&output.combined),
        RunOptions::default(),
    )
}

fn resolve_tsc_command(args: &[String]) -> Result<Command> {
    if let Some(local) = resolve_local_tsc_command(args)? {
        return Ok(local);
    }

    if tool_exists("tsc") {
        return Ok(resolved_command("tsc"));
    }

    anyhow::bail!(
        "failed to locate a real TypeScript compiler; install `typescript` in the repo or put `tsc` on PATH. cx will not fall back to `npx tsc`."
    )
}

fn resolve_local_tsc_command(args: &[String]) -> Result<Option<Command>> {
    let cwd = env::current_dir().context("failed to resolve current directory")?;
    for root in local_search_roots(args, &cwd) {
        if let Some(command) = local_tsc_command_from_root(&root) {
            return Ok(Some(command));
        }
    }
    Ok(None)
}

fn local_search_roots(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    if let Some(project_root) = project_search_root(args, cwd) {
        roots.insert(project_root);
    }
    roots.insert(cwd.to_path_buf());
    roots.into_iter().collect()
}

fn project_search_root(args: &[String], cwd: &Path) -> Option<PathBuf> {
    let raw = project_path_arg(args)?;
    let joined = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };

    if joined.is_dir() {
        return Some(joined);
    }
    if joined.extension().is_some() {
        return joined.parent().map(Path::to_path_buf);
    }
    Some(joined)
}

fn project_path_arg(args: &[String]) -> Option<PathBuf> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-p" || arg == "--project" {
            if let Some(value) = args.get(index + 1) {
                return Some(PathBuf::from(value));
            }
            return None;
        }
        if let Some(value) = arg.strip_prefix("--project=") {
            return Some(PathBuf::from(value));
        }
        index += 1;
    }
    None
}

fn local_tsc_command_from_root(root: &Path) -> Option<Command> {
    for dir in root.ancestors() {
        if let Some(shim) = local_tsc_shim(dir) {
            return Some(Command::new(shim));
        }
        if let Some(script) = local_typescript_script(dir) {
            let mut command = resolved_command("node");
            command.arg(script);
            return Some(command);
        }
    }
    None
}

fn local_tsc_shim(root: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates = [
        root.join("node_modules").join(".bin").join("tsc.cmd"),
        root.join("node_modules").join(".bin").join("tsc"),
    ];
    #[cfg(not(windows))]
    let candidates = [root.join("node_modules").join(".bin").join("tsc")];

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn local_typescript_script(root: &Path) -> Option<PathBuf> {
    let bin_script = root
        .join("node_modules")
        .join("typescript")
        .join("bin")
        .join("tsc");
    if bin_script.is_file() {
        return Some(bin_script);
    }

    let lib_script = root
        .join("node_modules")
        .join("typescript")
        .join("lib")
        .join("tsc.js");
    lib_script.is_file().then_some(lib_script)
}

fn filter_tsc_output(output: &str) -> Option<String> {
    struct TsError {
        file: String,
        line: usize,
        code: String,
        message: String,
        context: Vec<String>,
    }

    let lines: Vec<&str> = output.lines().collect();
    let mut errors = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];
        if let Some(captures) = tsc_error_re().captures(line) {
            let mut error = TsError {
                file: captures[1].to_string(),
                line: captures[2].parse().unwrap_or(0),
                code: captures[5].to_string(),
                message: captures[6].to_string(),
                context: Vec::new(),
            };
            index += 1;
            while index < lines.len() {
                let next = lines[index];
                if !next.is_empty()
                    && (next.starts_with("  ") || next.starts_with('\t'))
                    && !tsc_error_re().is_match(next)
                {
                    error.context.push(next.trim().to_string());
                    index += 1;
                } else {
                    break;
                }
            }
            errors.push(error);
        } else {
            index += 1;
        }
    }

    if errors.is_empty() {
        if output.contains("Found 0 errors") {
            return Some("TypeScript: No errors found".to_string());
        }
        return None;
    }

    let mut by_file: HashMap<String, Vec<&TsError>> = HashMap::new();
    let mut by_code: HashMap<String, usize> = HashMap::new();
    for error in &errors {
        by_file.entry(error.file.clone()).or_default().push(error);
        *by_code.entry(error.code.clone()).or_insert(0) += 1;
    }

    let mut code_counts: Vec<_> = by_code.iter().collect();
    code_counts.sort_by(|left, right| right.1.cmp(left.1));
    let mut files_sorted: Vec<_> = by_file.iter().collect();
    files_sorted.sort_by_key(|(_, errors)| std::cmp::Reverse(errors.len()));

    let mut result = String::new();
    result.push_str(&format!(
        "TypeScript: {} errors in {} files\n",
        errors.len(),
        by_file.len()
    ));
    result.push_str("═══════════════════════════════════════\n");
    if code_counts.len() > 1 {
        let summary = code_counts
            .iter()
            .take(5)
            .map(|(code, count)| format!("{code} ({count}x)"))
            .collect::<Vec<_>>()
            .join(", ");
        result.push_str(&format!("Top codes: {summary}\n\n"));
    }

    for (file, file_errors) in files_sorted {
        result.push_str(&format!("{file} ({} errors)\n", file_errors.len()));
        for error in file_errors {
            result.push_str(&format!(
                "  L{}: {} {}\n",
                error.line,
                error.code,
                truncate(&error.message, 120)
            ));
            for context in &error.context {
                result.push_str(&format!("    {}\n", truncate(context, 120)));
            }
        }
        result.push('\n');
    }

    Some(result.trim().to_string())
}

fn tsc_error_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(.+?)\((\d+),(\d+)\):\s+(error|warning)\s+(TS\d+):\s+(.+)$")
            .expect("tsc regex")
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn filter_tsc_output_groups_errors() {
        let output = "src/app.ts(10,5): error TS2322: Type 'string' is not assignable to type 'number'.\nsrc/lib.ts(5,1): error TS2339: Property 'x' does not exist on type 'Y'.";
        let filtered = filter_tsc_output(output).unwrap();
        assert!(filtered.contains("2 errors in 2 files"));
        assert!(filtered.contains("TS2322"));
    }

    #[test]
    fn filter_tsc_output_preserves_non_diagnostic_output() {
        let output = "Version 5.4.5";
        assert_eq!(filter_tsc_output(output), None);
    }

    #[test]
    fn project_path_arg_supports_short_and_long_flags() {
        assert_eq!(
            project_path_arg(&["-p".to_string(), "tsconfig.json".to_string()]),
            Some(PathBuf::from("tsconfig.json"))
        );
        assert_eq!(
            project_path_arg(&["--project=web/tsconfig.json".to_string()]),
            Some(PathBuf::from("web/tsconfig.json"))
        );
        assert_eq!(
            project_path_arg(&["--project".to_string(), "app".to_string()]),
            Some(PathBuf::from("app"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_uses_fake_tsc_binary() {
        crate::support::test_support::with_fake_path(
            &[(
                "tsc",
                "#!/bin/sh\ncat <<'EOF'\nsrc/app.ts(10,5): error TS2322: Type 'string' is not assignable to type 'number'.\nEOF\nexit 2\n",
            )],
            || {
                let output = run(&[]).unwrap();
                assert_eq!(output.exit_code, 2);
                assert!(output.stdout.contains("TS2322"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_prefers_repo_local_tsc_from_project_path() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("workspace").join("node");
        fs::create_dir_all(project.join("node_modules/.bin")).unwrap();
        fs::write(project.join("tsconfig.json"), "{ }\n").unwrap();
        crate::support::test_support::write_executable(
            &project.join("node_modules/.bin"),
            "tsc",
            "#!/bin/sh\nprintf 'Found 0 errors. Watching for file changes.\\n'\n",
        );

        crate::support::test_support::with_fake_path_only(
            &[("npx", "#!/bin/sh\necho npx-was-invoked >&2\nexit 9\n")],
            || {
                let output = run(&[
                    "-p".to_string(),
                    project.join("tsconfig.json").display().to_string(),
                    "--noEmit".to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert_eq!(output.stdout, "TypeScript: No errors found");
                assert!(output.stderr.is_empty());
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_prefers_repo_local_tsc_over_path_tsc() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("workspace");
        let app = project.join("packages/app");
        fs::create_dir_all(project.join("node_modules/.bin")).unwrap();
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("tsconfig.json"), "{ }\n").unwrap();
        crate::support::test_support::write_executable(
            &project.join("node_modules/.bin"),
            "tsc",
            "#!/bin/sh\nprintf 'Found 0 errors.\\n'\n",
        );

        crate::support::test_support::with_fake_path(
            &[(
                "tsc",
                "#!/bin/sh\necho global-tsc-should-not-run >&2\nexit 7\n",
            )],
            || {
                let output = run(&[
                    "--project".to_string(),
                    app.join("tsconfig.json").display().to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert_eq!(output.stdout, "TypeScript: No errors found");
                assert!(output.stderr.is_empty());
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_errors_when_no_tsc_exists() {
        let temp = tempdir().unwrap();
        let args_file = temp.path().join("npx-args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'npx-should-not-run\\n' >&2\nexit 9\n",
            args_file.display()
        );

        crate::support::test_support::with_fake_path_only(&[("npx", &script)], || {
            let error = run(&["-p".to_string(), "tsconfig.json".to_string()]).unwrap_err();
            assert!(error
                .to_string()
                .contains("failed to locate a real TypeScript compiler"));
        });

        assert!(!args_file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn run_uses_typescript_lib_script_when_bin_shim_is_missing() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("workspace");
        let typescript_lib = project.join("node_modules/typescript/lib");
        fs::create_dir_all(&typescript_lib).unwrap();
        fs::write(project.join("tsconfig.json"), "{ }\n").unwrap();
        crate::support::test_support::write_executable(
            &typescript_lib,
            "tsc.js",
            "#!/bin/sh\nprintf 'Found 0 errors.\\n'\n",
        );

        crate::support::test_support::with_fake_path_only(
            &[(
                "node",
                "#!/bin/sh\nscript=\"$1\"\nshift\nexec \"$script\" \"$@\"\n",
            )],
            || {
                let output = run(&[
                    "--project".to_string(),
                    project.join("tsconfig.json").display().to_string(),
                    "--noEmit".to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert_eq!(output.stdout, "TypeScript: No errors found");
                assert!(output.stderr.is_empty());
            },
        );
    }
}
