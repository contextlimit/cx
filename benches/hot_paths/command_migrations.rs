use std::hint::black_box;

use criterion::Criterion;
use cx::commands::{container, git, ls};

use crate::hot_paths::config::configure_process_group;
use crate::support;

pub fn bench_command_migrations(c: &mut Criterion) {
    let temp = support::ProjectTempDir::new("command-migrations");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let raw_ls = support::ls_output_fixture(180);
    let raw_docker_ps = support::docker_ps_output_fixture(60);
    let git_status = support::git_status_porcelain_fixture();
    let git_diff_stat = support::git_diff_stat_fixture();
    let git_diff = support::git_diff_fixture(240);
    let git_log = support::git_log_fixture(40);
    let ls_output_path = temp.path().join("ls.out");
    let docker_output_path = temp.path().join("docker-ps.out");
    let git_status_path = temp.path().join("git-status.out");
    let git_diff_stat_path = temp.path().join("git-diff-stat.out");
    let git_diff_path = temp.path().join("git-diff.out");
    let git_log_path = temp.path().join("git-log.out");
    support::write_file(&ls_output_path, &raw_ls);
    support::write_file(&docker_output_path, &raw_docker_ps);
    support::write_file(&git_status_path, &git_status);
    support::write_file(&git_diff_stat_path, &git_diff_stat);
    support::write_file(&git_diff_path, &git_diff);
    support::write_file(&git_log_path, &git_log);
    support::write_executable(
        &bin,
        "ls",
        &support::output_script(Some(&ls_output_path), None, 0),
    );
    support::write_executable(
        &bin,
        "docker",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"ps\" ]; then\n/bin/cat {}\nelse\nexit 1\nfi\n",
            support::shell_quote(&docker_output_path)
        ),
    );
    support::write_executable(
        &bin,
        "git",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  status) if [ \"$2\" = \"--porcelain\" ]; then /bin/cat {}; else printf ' M src/lib.rs\\n'; fi ;;\n  diff) if [ \"$2\" = \"--stat\" ]; then /bin/cat {}; else /bin/cat {}; fi ;;\n  log) /bin/cat {} ;;\n  *) exit 9 ;;\nesac\n",
            support::shell_quote(&git_status_path),
            support::shell_quote(&git_diff_stat_path),
            support::shell_quote(&git_diff_path),
            support::shell_quote(&git_log_path)
        ),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let compact = ls::run(&[]).unwrap();
    support::assert_reduction(
        "ls capture migration",
        &raw_ls,
        &compact.stdout,
        0.35,
        &["Summary:", "src/", "file_000.rs"],
    );
    let docker_ps = container::run_docker_ps(&[]).unwrap();
    support::assert_reduction(
        "docker ps capture migration",
        &raw_docker_ps,
        &docker_ps.stdout,
        0.35,
        &["[docker] 60 containers", "web_000", "... +45 more"],
    );
    let git_diff_compact = git::run_diff(&[]).unwrap();
    support::assert_reduction(
        "git diff capture migration",
        &git_diff,
        &git_diff_compact.stdout,
        0.45,
        &["src/lib.rs", "+240 -240", "[full diff:"],
    );

    let mut group = c.benchmark_group("command_migrations");
    configure_process_group(&mut group);
    group.bench_function("ls_capture", |b| b.iter(|| ls::run(black_box(&[]))));
    group.bench_function("docker_ps_capture", |b| {
        b.iter(|| container::run_docker_ps(black_box(&[])))
    });
    group.bench_function("git_status_capture", |b| {
        b.iter(|| git::run_status(black_box(&[])))
    });
    group.bench_function("git_diff_capture", |b| {
        b.iter(|| git::run_diff(black_box(&[])))
    });
    group.bench_function("git_log_capture", |b| {
        b.iter(|| git::run_log(black_box(&[])))
    });
    group.finish();
}
