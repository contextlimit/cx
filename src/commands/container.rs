use anyhow::Result;

use crate::support::logs::summarize_logs;
use crate::support::runner::{
    append_failure_hint, capture, run_filtered, ProxyOutcome, RunOptions,
};
use crate::support::utils::{fallback_window, resolved_command};

pub fn run_docker_ps(args: &[String]) -> Result<ProxyOutcome> {
    let custom_format = has_docker_format_arg(args);
    let (global_args, ps_args) = split_docker_global_args(args);
    let mut output = {
        let mut cmd = resolved_command("docker");
        for arg in &global_args {
            cmd.arg(arg);
        }
        cmd.arg("ps");
        if custom_format {
            for arg in &ps_args {
                cmd.arg(arg);
            }
        } else {
            cmd.args([
                "--format",
                "{{.ID}}\t{{.Names}}\t{{.Status}}\t{{.Image}}\t{{.Ports}}",
            ]);
            for arg in &ps_args {
                cmd.arg(arg);
            }
        }
        capture(cmd, "docker")
    }?;

    let exit_code = output.exit_code;
    if exit_code != 0 {
        let failure_hint = output.failure_artifact_hint("docker");
        let observation = output.observation("docker ps");
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

    Ok(ProxyOutcome {
        stdout: if custom_format {
            fallback_window(&output.stdout, 12, 28)
        } else {
            format_docker_ps(&output.stdout)
        },
        stderr: String::new(),
        exit_code,
        observation: None,
    }
    .with_observation(output.observation("docker ps"))
    .with_expansion_reason("container-summary"))
}

fn has_docker_format_arg(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--format" || arg.starts_with("--format="))
}

fn split_docker_global_args(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut global_args = Vec::new();
    let mut command_args = Vec::new();
    let mut pending_global_value = false;

    for arg in args {
        if pending_global_value {
            global_args.push(arg.clone());
            pending_global_value = false;
            continue;
        }

        match docker_global_arg_kind(arg) {
            Some(DockerGlobalArgKind::ConsumesNext) => {
                global_args.push(arg.clone());
                pending_global_value = true;
            }
            Some(DockerGlobalArgKind::Standalone) => global_args.push(arg.clone()),
            None => command_args.push(arg.clone()),
        }
    }

    (global_args, command_args)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockerGlobalArgKind {
    Standalone,
    ConsumesNext,
}

fn docker_global_arg_kind(arg: &str) -> Option<DockerGlobalArgKind> {
    match arg {
        "--config" | "-c" | "--context" | "-H" | "--host" | "-l" | "--log-level"
        | "--tlscacert" | "--tlscert" | "--tlskey" => Some(DockerGlobalArgKind::ConsumesNext),
        "-D" | "--debug" | "--tls" | "--tlsverify" | "-v" | "--version" => {
            Some(DockerGlobalArgKind::Standalone)
        }
        _ if arg.starts_with("--config=")
            || arg.starts_with("--context=")
            || arg.starts_with("--host=")
            || arg.starts_with("--log-level=")
            || arg.starts_with("--tlscacert=")
            || arg.starts_with("--tlscert=")
            || arg.starts_with("--tlskey=")
            || (arg.starts_with("-c") && arg.len() > 2)
            || (arg.starts_with("-H") && arg.len() > 2)
            || (arg.starts_with("-l") && arg.len() > 2) =>
        {
            Some(DockerGlobalArgKind::Standalone)
        }
        _ => None,
    }
}

pub fn run_docker_logs(container: &str, args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("docker");
    let (global_args, logs_args) = split_docker_global_args(args);
    for arg in &global_args {
        cmd.arg(arg);
    }
    cmd.args(["logs", "--tail", "100"]);
    for arg in &logs_args {
        cmd.arg(arg);
    }
    cmd.arg(container);

    run_filtered(
        cmd,
        "docker",
        |output| {
            Some(format!(
                "[docker] Logs for {container}:\n{}",
                summarize_logs(&output.stdout)
            ))
        },
        RunOptions::stdout_only().early_exit_on_failure(),
    )
}

pub fn run_kubectl_logs(pod: &str, args: &[String]) -> Result<ProxyOutcome> {
    let mut cmd = resolved_command("kubectl");
    cmd.args(["logs", "--tail", "100"]);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg(pod);

    run_filtered(
        cmd,
        "kubectl",
        |output| {
            Some(format!(
                "Logs for {pod}:\n{}",
                summarize_logs(&output.stdout)
            ))
        },
        RunOptions::stdout_only().early_exit_on_failure(),
    )
}

fn format_docker_ps(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.is_empty() {
        return "[docker] 0 containers".to_string();
    }

    let mut result = format!("[docker] {} containers:\n", lines.len());
    for line in lines.iter().take(15) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let id = &parts[0][..parts[0].len().min(12)];
        let name = parts[1];
        let image = parts[3].split('/').next_back().unwrap_or(parts[3]);
        let ports = compact_ports(parts.get(4).copied().unwrap_or(""));
        if ports == "-" {
            result.push_str(&format!("  {id} {name} ({image})\n"));
        } else {
            result.push_str(&format!("  {id} {name} ({image}) [{ports}]\n"));
        }
    }
    if lines.len() > 15 {
        result.push_str(&format!("  ... +{} more", lines.len() - 15));
    }
    result.trim_end().to_string()
}

fn compact_ports(ports: &str) -> String {
    if ports.trim().is_empty() {
        return "-".to_string();
    }
    let port_numbers: Vec<&str> = ports
        .split(',')
        .filter_map(|entry| {
            entry
                .split("->")
                .next()
                .and_then(|part| part.split(':').next_back())
        })
        .collect();
    if port_numbers.len() <= 3 {
        port_numbers.join(", ")
    } else {
        format!(
            "{}, ... +{}",
            port_numbers[..2].join(", "),
            port_numbers.len() - 2
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    #[test]
    fn format_docker_ps_compacts_structured_output() {
        let raw = "abcd1234abcd\tweb\tUp 2 hours\tnginx:latest\t0.0.0.0:80->80/tcp\n";
        let formatted = format_docker_ps(raw);
        assert!(formatted.contains("[docker] 1 containers"));
        assert!(formatted.contains("web"));
        assert!(formatted.contains("80"));
    }

    #[cfg(unix)]
    #[test]
    fn run_docker_ps_accepts_custom_format_args() {
        crate::support::test_support::with_fake_path(
            &[(
                "docker",
                "#!/bin/sh\nif [ \"$1\" = \"ps\" ]; then\nprintf 'abc web Up nginx 80/tcp\\n'\nelse\nexit 1\nfi\n",
            )],
            || {
                let output = run_docker_ps(&[
                    "--format".to_string(),
                    "{{.ID}} {{.Names}} {{.Status}} {{.Image}} {{.Ports}}".to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("abc web Up nginx 80/tcp"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_docker_ps_stores_failure_artifact() {
        let temp = tempdir().unwrap();
        let bin = temp.path().join("bin");
        let home = temp.path().join("home");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        crate::support::test_support::write_executable(
            &bin,
            "docker",
            "#!/bin/sh\nprintf 'Cannot connect to the Docker daemon\\n' >&2\nexit 1\n",
        );

        crate::support::test_support::with_env_vars(
            &[
                ("PATH", Some(bin.to_string_lossy().as_ref())),
                ("HOME", Some(home.to_string_lossy().as_ref())),
                ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
                ("CX_TOOL_FALLBACK_PATHS", None),
            ],
            || {
                let output = run_docker_ps(&[]).unwrap();
                assert_eq!(output.exit_code, 1);
                assert!(output.stderr.contains("Docker daemon"));
                assert!(output
                    .stdout
                    .contains("[full output: ~/.cx/cache/failures/docker/"));
                let artifact_dir = home.join(".cx/cache/failures/docker");
                assert_eq!(fs::read_dir(artifact_dir).unwrap().count(), 1);
            },
        );
    }

    #[test]
    fn split_docker_global_args_extracts_context_flags() {
        let (global, command) = split_docker_global_args(&[
            "--context".to_string(),
            "barracks".to_string(),
            "--all".to_string(),
            "--filter".to_string(),
            "name=sample".to_string(),
            "--format".to_string(),
            "{{.Names}}".to_string(),
        ]);
        assert_eq!(global, vec!["--context", "barracks"]);
        assert_eq!(
            command,
            vec!["--all", "--filter", "name=sample", "--format", "{{.Names}}"]
        );
    }

    #[test]
    fn split_docker_global_args_extracts_inline_context_flags() {
        let (global, command) =
            split_docker_global_args(&["--context=barracks".to_string(), "--all".to_string()]);
        assert_eq!(global, vec!["--context=barracks"]);
        assert_eq!(command, vec!["--all"]);
    }

    #[cfg(unix)]
    #[test]
    fn run_docker_ps_places_context_before_subcommand() {
        let temp = tempdir().unwrap();
        let args_file = temp.path().join("docker-args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'abcd1234abcd\\tweb\\tUp\\tnginx:latest\\t0.0.0.0:80->80/tcp\\n'\n",
            args_file.display()
        );
        crate::support::test_support::with_fake_path(&[("docker", &script)], || {
            let output = run_docker_ps(&[
                "--context".to_string(),
                "barracks".to_string(),
                "--all".to_string(),
                "--filter".to_string(),
                "name=sample".to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 0);
        });
        let args = fs::read_to_string(args_file).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(
            lines,
            vec![
                "--context",
                "barracks",
                "ps",
                "--format",
                "{{.ID}}\t{{.Names}}\t{{.Status}}\t{{.Image}}\t{{.Ports}}",
                "--all",
                "--filter",
                "name=sample"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_docker_ps_returns_without_waiting_for_descendant_stdout_to_close() {
        crate::support::test_support::with_fake_path(
            &[(
                "docker",
                "#!/bin/sh\nif [ \"$1\" = \"ps\" ]; then\n(sleep 1) &\nprintf 'abcd1234abcd\\tweb\\tUp\\tnginx:latest\\t0.0.0.0:80->80/tcp\\n'\nelse\nexit 1\nfi\n",
            )],
            || {
                let start = Instant::now();
                let output = run_docker_ps(&[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("[docker] 1 containers"));
                assert!(output.stdout.contains("web"));
                assert!(start.elapsed() < Duration::from_millis(700));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_docker_logs_places_context_before_subcommand() {
        let temp = tempdir().unwrap();
        let args_file = temp.path().join("docker-args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'ERROR: boom\\n'\n",
            args_file.display()
        );
        crate::support::test_support::with_fake_path(&[("docker", &script)], || {
            let output = run_docker_logs(
                "container-1",
                &[
                    "--context=barracks".to_string(),
                    "--since".to_string(),
                    "1h".to_string(),
                ],
            )
            .unwrap();
            assert_eq!(output.exit_code, 0);
        });
        let args = fs::read_to_string(args_file).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(
            lines,
            vec![
                "--context=barracks",
                "logs",
                "--tail",
                "100",
                "--since",
                "1h",
                "container-1"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_docker_logs_uses_fake_binary() {
        crate::support::test_support::with_fake_path(
            &[(
                "docker",
                "#!/bin/sh\nif [ \"$1\" = \"logs\" ]; then\ncat <<'EOF'\nERROR: boom\nERROR: boom\nEOF\nelse\nexit 1\nfi\n",
            )],
            || {
                let output = run_docker_logs("web", &[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("Log Summary"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_kubectl_logs_uses_fake_binary() {
        crate::support::test_support::with_fake_path(
            &[(
                "kubectl",
                "#!/bin/sh\ncat <<'EOF'\nWARN: retry\nWARN: retry\nEOF\n",
            )],
            || {
                let output = run_kubectl_logs("pod-1", &[]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("WARNINGS"));
            },
        );
    }
}
