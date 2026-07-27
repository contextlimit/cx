use anyhow::Result;

use crate::support::command_output;
use crate::support::insights::{OutputObservation, TextMetrics};
use crate::support::runner::{run_filtered, CommandOutput, ProxyOutcome, RunOptions};
use crate::support::utils::resolved_command;

pub fn run_build(args: &[String]) -> Result<ProxyOutcome> {
    let parsed = parse_build_args(args);
    if parsed.targets.len() > 1 {
        return run_multi_target_build(&parsed);
    }
    run_single_build(args)
}

fn run_single_build(args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("cmake");
    cmd.arg("--build");
    for arg in args {
        cmd.arg(arg);
    }

    Ok(
        run_filtered(cmd, "cmake", filter_cmake_build, RunOptions::default())?
            .with_expansion_reason("build-result-summary"),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedBuildArgs {
    cmake_args: Vec<String>,
    native_args: Vec<String>,
    targets: Vec<String>,
    split_safe: bool,
}

fn parse_build_args(args: &[String]) -> ParsedBuildArgs {
    let mut cmake_args = Vec::new();
    let mut native_args = Vec::new();
    let mut targets = Vec::new();
    let mut split_safe = true;
    let mut index = 0usize;
    let mut in_native_args = false;

    while index < args.len() {
        let arg = &args[index];
        if in_native_args {
            native_args.push(arg.clone());
            index += 1;
            continue;
        }

        if arg == "--" {
            in_native_args = true;
            index += 1;
            continue;
        }

        if arg == "--target" || arg == "-t" {
            index += 1;
            let mut saw_value = false;
            while index < args.len() {
                let value = &args[index];
                if value == "--" || value.starts_with('-') {
                    break;
                }
                saw_value = true;
                targets.push(value.clone());
                index += 1;
            }
            if !saw_value {
                split_safe = false;
                cmake_args.push(arg.clone());
            }
            continue;
        }

        if let Some(value) = arg.strip_prefix("--target=") {
            if value.is_empty() {
                split_safe = false;
                cmake_args.push(arg.clone());
            } else {
                targets.push(value.to_string());
            }
            index += 1;
            continue;
        }

        cmake_args.push(arg.clone());
        index += 1;
    }

    ParsedBuildArgs {
        cmake_args,
        native_args,
        targets,
        split_safe,
    }
}

fn run_multi_target_build(parsed: &ParsedBuildArgs) -> Result<ProxyOutcome> {
    if !parsed.split_safe {
        return run_single_build(&reconstruct_raw_args(parsed));
    }

    let total_targets = parsed.targets.len();
    let mut stdout_sections = Vec::new();
    let mut stderr_sections = Vec::new();
    let mut raw_metrics = TextMetrics::default();

    for (index, target) in parsed.targets.iter().enumerate() {
        let args = target_build_args(parsed, target);
        let outcome = run_single_build(&args)?;
        raw_metrics = raw_metrics.plus(
            outcome
                .observation
                .as_ref()
                .map(|observation| observation.metrics)
                .unwrap_or_else(|| TextMetrics::from_text(&outcome.stdout)),
        );
        let body = outcome_body(&outcome.stdout);
        stdout_sections.push(render_target_section(target, body));
        if !outcome.stderr.trim().is_empty() {
            stderr_sections.push(render_target_section(target, outcome.stderr.trim()));
        }
        if outcome.exit_code != 0 {
            let mut stdout = format!(
                "cmake build: failed while building target `{}` ({}/{})",
                target,
                index + 1,
                total_targets
            );
            if !stdout_sections.is_empty() {
                stdout.push('\n');
                stdout.push_str(&stdout_sections.join("\n\n"));
            }
            return Ok(ProxyOutcome {
                stdout,
                stderr: stderr_sections.join("\n\n"),
                exit_code: outcome.exit_code,
                observation: None,
            }
            .with_observation(OutputObservation::from_metrics(
                "cmake multi-target",
                raw_metrics,
            ))
            .with_expansion_reason("build-result-summary"));
        }
    }

    let mut stdout = format!("cmake build: ok ({} targets)", total_targets);
    if !stdout_sections.is_empty() {
        stdout.push('\n');
        stdout.push_str(&stdout_sections.join("\n\n"));
    }

    Ok(ProxyOutcome {
        stdout,
        stderr: stderr_sections.join("\n\n"),
        exit_code: 0,
        observation: None,
    }
    .with_observation(OutputObservation::from_metrics(
        "cmake multi-target",
        raw_metrics,
    ))
    .with_expansion_reason("build-result-summary"))
}

fn reconstruct_raw_args(parsed: &ParsedBuildArgs) -> Vec<String> {
    let mut args = parsed.cmake_args.clone();
    if !parsed.targets.is_empty() {
        args.push("--target".to_string());
        args.extend(parsed.targets.clone());
    }
    if !parsed.native_args.is_empty() {
        args.push("--".to_string());
        args.extend(parsed.native_args.clone());
    }
    args
}

fn target_build_args(parsed: &ParsedBuildArgs, target: &str) -> Vec<String> {
    let mut args = parsed.cmake_args.clone();
    args.push("--target".to_string());
    args.push(target.to_string());
    if !parsed.native_args.is_empty() {
        args.push("--".to_string());
        args.extend(parsed.native_args.clone());
    }
    args
}

fn outcome_body(stdout: &str) -> &str {
    if let Some(rest) = stdout.strip_prefix("cmake build: ok\n") {
        rest.trim()
    } else if stdout.trim() == "cmake build: ok" {
        ""
    } else {
        stdout.trim()
    }
}

fn render_target_section(target: &str, body: &str) -> String {
    if body.is_empty() {
        format!("[target] {target}")
    } else {
        format!("[target] {target}\n{body}")
    }
}

fn filter_cmake_build(output: &CommandOutput) -> Option<String> {
    command_output::summarize_cmake_build(&output.combined, output.exit_code)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn filter_cmake_build_keeps_success_summary() {
        let output = CommandOutput::from_combined(
            "[1/2] Building CXX object app.cpp.o\n[2/2] Linking CXX executable Dock\nBuilt target Dock\n",
            0,
        );
        let filtered = filter_cmake_build(&output).unwrap();
        assert!(filtered.contains("cmake build: ok"));
        assert!(filtered.contains("Built target Dock"));
    }

    #[test]
    fn filter_cmake_build_keeps_failure_diagnostics() {
        let output = CommandOutput::from_combined(
            "[1/2] Building CXX object app.cpp.o\nFAILED: app.cpp.o\nsrc/app.cpp:10:5: error: unknown type name 'Dock'\nninja: build stopped: subcommand failed.\n",
            1,
        );
        let filtered = filter_cmake_build(&output).unwrap();
        assert!(filtered.contains("cmake build: failed"));
        assert!(filtered.contains("unknown type name"));
        assert!(filtered.contains("ninja: build stopped"));
    }

    #[cfg(unix)]
    #[test]
    fn run_build_passes_args_to_cmake_build() {
        let temp = tempdir().unwrap();
        let args_file = temp.path().join("cmake-args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'Built target Dock\\n'\n",
            args_file.display()
        );
        crate::support::test_support::with_fake_path(&[("cmake", &script)], || {
            let output = run_build(&[
                "build".to_string(),
                "--target".to_string(),
                "Dock".to_string(),
                "--".to_string(),
                "-j4".to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("cmake build: ok"));
        });
        let args = fs::read_to_string(args_file).unwrap();
        assert!(args.contains("--build"));
        assert!(args.contains("build"));
        assert!(args.contains("--target"));
        assert!(args.contains("Dock"));
        assert!(args.contains("-j4"));
    }

    #[test]
    fn parse_build_args_collects_multi_target_form() {
        let parsed = parse_build_args(&[
            "build".to_string(),
            "--target".to_string(),
            "alpha".to_string(),
            "beta".to_string(),
            "gamma".to_string(),
            "-j".to_string(),
            "8".to_string(),
            "--".to_string(),
            "VERBOSE=1".to_string(),
        ]);
        assert!(parsed.split_safe);
        assert_eq!(
            parsed.targets,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
        assert_eq!(
            parsed.cmake_args,
            vec!["build".to_string(), "-j".to_string(), "8".to_string()]
        );
        assert_eq!(parsed.native_args, vec!["VERBOSE=1".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn run_build_splits_multi_target_invocations() {
        let temp = tempdir().unwrap();
        let args_file = temp.path().join("cmake-args.txt");
        let script = format!(
            "#!/bin/sh\n\
            printf -- '---\\n' >> '{log}'\n\
            target=''\n\
            for arg in \"$@\"; do\n\
              printf '%s\\n' \"$arg\" >> '{log}'\n\
            done\n\
            while [ $# -gt 0 ]; do\n\
              if [ \"$1\" = \"--target\" ]; then\n\
                shift\n\
                target=\"$1\"\n\
                continue\n\
              fi\n\
              shift\n\
            done\n\
            printf 'Built target %s\\n' \"$target\" \n",
            log = args_file.display()
        );
        crate::support::test_support::with_fake_path(&[("cmake", &script)], || {
            let output = run_build(&[
                "build".to_string(),
                "--target".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                "-j".to_string(),
                "8".to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("cmake build: ok (3 targets)"));
            assert!(output.stdout.contains("[target] alpha"));
            assert!(output.stdout.contains("[target] beta"));
            assert!(output.stdout.contains("[target] gamma"));
        });
        let args = fs::read_to_string(args_file).unwrap();
        assert_eq!(args.matches("---\n").count(), 3);
        assert_eq!(args.matches("--target\n").count(), 3);
        assert!(args.contains("build\n-j\n8\n--target\nalpha\n"));
        assert!(args.contains("build\n-j\n8\n--target\nbeta\n"));
        assert!(args.contains("build\n-j\n8\n--target\ngamma\n"));
    }

    #[cfg(unix)]
    #[test]
    fn run_build_stops_at_first_failed_target() {
        let temp = tempdir().unwrap();
        let args_file = temp.path().join("cmake-args.txt");
        let script = format!(
            "#!/bin/sh\n\
            printf -- '---\\n' >> '{log}'\n\
            target=''\n\
            while [ $# -gt 0 ]; do\n\
              printf '%s\\n' \"$1\" >> '{log}'\n\
              if [ \"$1\" = \"--target\" ]; then\n\
                shift\n\
                target=\"$1\"\n\
                printf '%s\\n' \"$1\" >> '{log}'\n\
              fi\n\
              shift\n\
            done\n\
            if [ \"$target\" = \"beta\" ]; then\n\
              printf 'FAILED: %s\\n' \"$target\"\n\
              printf 'error: boom\\n'\n\
              exit 1\n\
            fi\n\
            printf 'Built target %s\\n' \"$target\" \n",
            log = args_file.display()
        );
        crate::support::test_support::with_fake_path(&[("cmake", &script)], || {
            let output = run_build(&[
                "build".to_string(),
                "--target".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 1);
            assert!(output
                .stdout
                .contains("cmake build: failed while building target `beta` (2/3)"));
            assert!(output.stdout.contains("[target] alpha"));
            assert!(output.stdout.contains("[target] beta"));
            assert!(!output.stdout.contains("[target] gamma"));
            assert!(output.stdout.contains("error: boom"));
        });
        let args = fs::read_to_string(args_file).unwrap();
        assert_eq!(args.matches("---\n").count(), 2);
        assert!(args.contains("alpha\n"));
        assert!(args.contains("beta\n"));
        assert!(!args.contains("gamma\n"));
    }
}
