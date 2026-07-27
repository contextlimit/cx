use std::hint::black_box;

use criterion::Criterion;
use cx::commands::container;

use crate::hot_paths::config::configure_process_group;
use crate::support;

struct LogWrapperBenchSetup {
    _env: support::EnvGuard,
    _temp: support::ProjectTempDir,
    docker_args: Vec<String>,
    kubectl_args: Vec<String>,
}

fn setup_log_wrapper_bench() -> LogWrapperBenchSetup {
    let temp = support::ProjectTempDir::new("log-wrappers");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw_logs = support::log_output_fixture(180);
    let logs_out = temp.path().join("logs.out");
    support::write_file(&logs_out, &raw_logs);
    support::write_executable(
        &bin,
        "docker",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"logs\" ]; then\n/bin/cat {}\nelse\nexit 1\nfi\n",
            support::shell_quote(&logs_out)
        ),
    );
    support::write_executable(
        &bin,
        "kubectl",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"logs\" ]; then\n/bin/cat {}\nelse\nexit 1\nfi\n",
            support::shell_quote(&logs_out)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let docker_args = Vec::new();
    let docker_once = container::run_docker_logs("web", &docker_args).unwrap();
    support::assert_reduction(
        "docker logs wrapper",
        &raw_logs,
        &docker_once.stdout,
        0.30,
        &["[docker] Logs for web", "Log Summary", "[ERRORS]"],
    );

    let kubectl_args = vec!["--namespace".to_string(), "default".to_string()];
    let kubectl_once = container::run_kubectl_logs("pod-1", &kubectl_args).unwrap();
    support::assert_reduction(
        "kubectl logs wrapper",
        &raw_logs,
        &kubectl_once.stdout,
        0.30,
        &["Logs for pod-1", "Log Summary", "[WARNINGS]"],
    );

    LogWrapperBenchSetup {
        _env,
        _temp: temp,
        docker_args,
        kubectl_args,
    }
}

pub fn bench_log_wrappers(c: &mut Criterion) {
    let setup = setup_log_wrapper_bench();
    let mut group = c.benchmark_group("log_wrappers");
    configure_process_group(&mut group);
    group.bench_function("docker_logs", |b| {
        b.iter(|| container::run_docker_logs(black_box("web"), black_box(&setup.docker_args)))
    });
    group.bench_function("kubectl_logs", |b| {
        b.iter(|| container::run_kubectl_logs(black_box("pod-1"), black_box(&setup.kubectl_args)))
    });
    group.finish();
}
