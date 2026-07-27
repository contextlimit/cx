use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use tempfile::tempdir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn install_script_installs_runtime_binary_and_wrapper() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let fake_bin = temp.path().join("cx");
    fs::create_dir_all(&home).unwrap();
    fs::write(&fake_bin, "#!/bin/sh\nprintf 'runtime ok\\n'\n").unwrap();
    make_executable(&fake_bin);

    let output = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", &fake_bin)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let install_path = home.join(".local/bin/cx");
    let runtime_path = home.join(".cx/bin/cx");
    let metadata = fs::symlink_metadata(&install_path).unwrap();
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(
        fs::read_to_string(&runtime_path).unwrap(),
        "#!/bin/sh\nprintf 'runtime ok\\n'\n"
    );
    assert_eq!(
        fs::read_to_string(&install_path).unwrap(),
        format!(
            "#!/usr/bin/env sh\nif [ ! -x \"{runtime}\" ]; then\n  echo \"cx wrapper: missing runtime binary at {runtime}\" >&2\n  exit 127\nfi\nexec \"{runtime}\" \"$@\"\n",
            runtime = runtime_path.display()
        )
    );
    assert!(home.join(".config/cx").is_dir());
    assert!(home.join(".cx/bin").is_dir());
    assert!(home.join(".cx/cache").is_dir());
    assert!(!home.join(".cache/cx").exists());

    let wrapper_run = Command::new(&install_path).output().unwrap();
    assert!(wrapper_run.status.success(), "{wrapper_run:?}");
    assert_eq!(String::from_utf8_lossy(&wrapper_run.stdout), "runtime ok\n");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not on PATH"));
}

#[cfg(unix)]
#[test]
fn reinstall_replaces_binaries_without_touching_insights_database() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let fake_bin = temp.path().join("cx");
    let db_path = home.join(".cx/db.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    fs::write(&db_path, b"telemetry must survive reinstall").unwrap();
    fs::write(&fake_bin, "#!/bin/sh\nprintf 'revision one\\n'\n").unwrap();
    make_executable(&fake_bin);

    run_install(&home, &fake_bin);
    fs::write(&fake_bin, "#!/bin/sh\nprintf 'revision two\\n'\n").unwrap();
    make_executable(&fake_bin);
    run_install(&home, &fake_bin);

    assert_eq!(
        fs::read(&db_path).unwrap(),
        b"telemetry must survive reinstall"
    );
    assert!(fs::read_to_string(home.join(".cx/bin/cx"))
        .unwrap()
        .contains("revision two"));
    for root in [home.join(".cx/bin"), home.join(".local/bin")] {
        assert!(fs::read_dir(root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp.")));
    }
}

#[cfg(unix)]
fn run_install(home: &std::path::Path, binary: &std::path::Path) {
    let output = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", binary)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[cfg(unix)]
#[test]
fn installed_wrapper_routes_auto_evidence_diff_without_compacting_patch() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    let patch_path = temp.path().join("expected.diff");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let raw_patch = (0..180)
        .map(|line| format!("+installed_exact_evidence_{line:03}\n"))
        .collect::<String>();
    fs::write(&patch_path, &raw_patch).unwrap();
    let git = bin.join("git");
    fs::write(
        &git,
        format!(
            "#!/bin/sh\nif [ \"$1\" = diff ]; then /bin/cat \"{}\"; exit 0; fi\nexit 9\n",
            patch_path.display()
        ),
    )
    .unwrap();
    make_executable(&git);

    let install = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", env!("CARGO_BIN_EXE_cx"))
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let installed = home.join(".local/bin/cx");
    let run = Command::new(&installed)
        .args(["--", "git", "evidence-diff", "HEAD~1..HEAD"])
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CX_DISABLE_INSIGHTS", "1")
        .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1")
        .output()
        .unwrap();

    assert!(run.status.success(), "{run:?}");
    assert_eq!(String::from_utf8(run.stdout).unwrap(), raw_patch);
    assert!(run.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn installed_wrapper_preserves_structured_source_lines_for_sed_and_rg() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    let fixture = temp.path().join("archive_fixture.rs");
    let (registry_fixture, registry_line) = write_registry_fixture(temp.path());
    let first_source = temp.path().join("first.cpp");
    let second_source = temp.path().join("second.cpp");
    let rg_output = temp.path().join("rg.out");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let source_line = structured_source_line();
    fs::write(&fixture, format!("before\n{source_line}\nafter\n")).unwrap();
    let first_content = (1..=300)
        .map(|line| format!("int first_{line:04}();\n"))
        .collect::<String>();
    let second_content = (1..=2_600)
        .map(|line| format!("int second_{line:04}();\n"))
        .collect::<String>();
    fs::write(&first_source, &first_content).unwrap();
    fs::write(&second_source, &second_content).unwrap();
    let expected_rg_output = format!("{}:2:        {source_line}\n", fixture.display());
    fs::write(&rg_output, &expected_rg_output).unwrap();
    let fake_rg = bin.join("rg");
    fs::write(
        &fake_rg,
        format!("#!/bin/sh\n/bin/cat '{}'\n", rg_output.display()),
    )
    .unwrap();
    make_executable(&fake_rg);

    let install = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", env!("CARGO_BIN_EXE_cx"))
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let installed = home.join(".local/bin/cx");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let sed = Command::new(&installed)
        .args(["--", "sed", "-n", "2,2p", fixture.to_str().unwrap()])
        .env("HOME", &home)
        .env("PATH", &path)
        .env("CX_DISABLE_INSIGHTS", "1")
        .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1")
        .output()
        .unwrap();
    assert!(sed.status.success(), "{sed:?}");
    assert_eq!(
        String::from_utf8(sed.stdout).unwrap(),
        format!("{source_line}\n")
    );

    assert_installed_registry_source_range(
        &installed,
        &home,
        &path,
        &registry_fixture,
        &registry_line,
    );

    let rg = Command::new(&installed)
        .args([
            "--",
            "rg",
            "-n",
            "-F",
            "rawBytes",
            fixture.to_str().unwrap(),
        ])
        .env("HOME", &home)
        .env("PATH", path)
        .env("CX_DISABLE_INSIGHTS", "1")
        .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1")
        .output()
        .unwrap();
    assert!(rg.status.success(), "{rg:?}");
    let stdout = String::from_utf8(rg.stdout).unwrap();
    assert_eq!(stdout, expected_rg_output);

    assert_installed_combined_source_ranges(
        &installed,
        &home,
        &first_source,
        &first_content,
        &second_source,
        &second_content,
    );
}

#[cfg(unix)]
fn write_registry_fixture(root: &std::path::Path) -> (PathBuf, String) {
    let path = root.join("command_registry.cpp");
    let description = "Set true when the current user request requires a new or bound plan before repository work. With no bound plan, sync_context returns createRequiredPlan with shouldStop and mustBindBeforeWork; only plan preflight, allowed plan-authoring files, and plan persistence may proceed until a plan is bound. ";
    let line = format!(
        "              {{\"description\", \"{}\"}}}},",
        description.repeat(2)
    );
    fs::write(&path, format!("before\n{line}\nafter\n")).unwrap();
    (path, line)
}

#[cfg(unix)]
fn assert_installed_registry_source_range(
    installed: &std::path::Path,
    home: &std::path::Path,
    path: &str,
    fixture: &std::path::Path,
    source_line: &str,
) {
    let output = Command::new(installed)
        .args(["--", "sed", "-n", "2,2p", fixture.to_str().unwrap()])
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_DISABLE_INSIGHTS", "1")
        .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{source_line}\n")
    );
}

#[cfg(unix)]
fn assert_installed_combined_source_ranges(
    installed: &std::path::Path,
    home: &std::path::Path,
    first_path: &std::path::Path,
    first_content: &str,
    second_path: &std::path::Path,
    second_content: &str,
) {
    let script = format!(
        "sed -n '1,260p' '{}'; sed -n '1,2500p' '{}'",
        first_path.display(),
        second_path.display(),
    );
    let output = Command::new(installed)
        .args(["--", "bash", "-lc", &script])
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_DISABLE_INSIGHTS", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let first = first_content
        .lines()
        .take(260)
        .collect::<Vec<_>>()
        .join("\n");
    let second = second_content
        .lines()
        .take(2_500)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{first}\n{second}\n")
    );
}

#[cfg(unix)]
#[test]
fn installed_wrapper_cmake_failure_keeps_terminal_cause_after_warning_flood() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let cmake = bin.join("cmake");
    fs::write(
        &cmake,
        r#"#!/bin/sh
i=0
while [ "$i" -lt 96 ]; do
  printf "ld: warning: ignoring duplicate libraries: '-lc++'\n"
  i=$((i + 1))
done
printf '%s\n' \
  'src/runtime.cpp:42:9: warning: ignoring return value of nodiscard function' \
  'The command could not be loaded, possibly because:' \
  '  * You intended to execute a .NET SDK command:' \
  '      A compatible .NET SDK was not found.' \
  'Requested SDK version: 8.0.125' \
  'make[2]: *** [generated/service.stamp] Error 145' \
  'make[1]: *** [all] Error 2' \
  'make: *** [all] Error 2'
exit 2
"#,
    )
    .unwrap();
    make_executable(&cmake);

    let install = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", env!("CARGO_BIN_EXE_cx"))
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let installed = home.join(".local/bin/cx");
    let run = Command::new(&installed)
        .args(["--", "cmake", "--build", "build", "--target", "service"])
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CX_DISABLE_INSIGHTS", "1")
        .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1")
        .output()
        .unwrap();

    assert_eq!(run.status.code(), Some(2), "{run:?}");
    let stdout = String::from_utf8(run.stdout).unwrap();
    assert!(stdout.contains("cmake build: failed (exit 2)"));
    assert!(stdout.contains("A compatible .NET SDK was not found."));
    assert!(stdout.contains("Requested SDK version: 8.0.125"));
    assert!(stdout.contains("Error 145"));
    assert!(stdout.contains("make: *** [all] Error 2"));
    assert!(stdout.contains("repeated warning lines suppressed"));
    assert_eq!(
        stdout
            .matches("ld: warning: ignoring duplicate libraries: '-lc++'")
            .count(),
        1,
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn installed_wrapper_preserves_passthrough_failures_in_artifacts() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let failing_tool = bin.join("demo-fail");
    fs::write(
        &failing_tool,
        "#!/bin/sh\nprintf 'native partial stdout\\n'\nprintf 'native stderr\\n' >&2\nexit 7\n",
    )
    .unwrap();
    make_executable(&failing_tool);

    let install = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", env!("CARGO_BIN_EXE_cx"))
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let installed = home.join(".local/bin/cx");
    let run = Command::new(&installed)
        .args(["--", "demo-fail", "alpha"])
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
        .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1")
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(7), "{run:?}");
    let stdout = String::from_utf8(run.stdout).unwrap();
    let stderr = String::from_utf8(run.stderr).unwrap();
    assert!(stdout.contains("native partial stdout"));
    assert!(stdout.contains("[full output: ~/.cx/cache/failures/passthrough/"));
    assert!(stderr.contains("native stderr"));

    let artifact_dir = home.join(".cx/cache/failures/passthrough");
    let artifacts = fs::read_dir(&artifact_dir)
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    let artifact = fs::read_to_string(artifacts[0].path()).unwrap();
    assert!(artifact.contains("native partial stdout"));
    assert!(artifact.contains("native stderr"));
}

#[cfg(unix)]
#[test]
fn installed_wrapper_preserves_native_auto_routes_and_silent_failures() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    let diff_argv = temp.path().join("diff-argv.txt");
    let find_argv = temp.path().join("find-argv.txt");
    let rg_argv = temp.path().join("rg-argv.txt");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();

    for (name, body) in [
        (
            "diff",
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_DIFF_ARGV_PATH\"\nprintf 'native-diff-executed\\n'\n",
        ),
        (
            "git",
            "#!/bin/sh\nif [ \"$1\" = diff ] && [ \"$2\" = --stat ]; then\nprintf 'git-diff-stat-executed\\n'\nexit 0\nfi\nprintf 'unexpected-git: %s\\n' \"$*\" >&2\nexit 97\n",
        ),
        (
            "find",
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_FIND_ARGV_PATH\"\nprintf 'build/image-service\\nbuild/identity-service\\n'\n",
        ),
        (
            "rg",
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_RG_ARGV_PATH\"\nprintf 'transform:translateY(50px)\\n'\n",
        ),
        ("silent-fail", "#!/bin/sh\nexit 1\n"),
    ] {
        let path = bin.join(name);
        fs::write(&path, body).unwrap();
        make_executable(&path);
    }

    let install = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", env!("CARGO_BIN_EXE_cx"))
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let installed = home.join(".local/bin/cx");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let settings = Command::new(&installed)
        .args([
            "insights",
            "settings",
            "--set",
            "record_invocations=true",
            "--set",
            "record_command_text=true",
            "--set",
            "record_failure_responses=true",
            "--set",
            "record_failures=true",
            "--set",
            "passthrough_unsupported_commands=true",
        ])
        .env("HOME", &home)
        .env("PATH", &path)
        .env("CX_INSIGHTS_DB_PATH", &db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert!(settings.status.success(), "{settings:?}");

    assert_installed_native_diff(&installed, &home, &path, &db_path, &diff_argv);
    assert_installed_native_find(&installed, &home, &path, &db_path, &find_argv);
    assert_installed_native_rg(&installed, &home, &path, &db_path, &rg_argv);
    assert_installed_silent_failure(&installed, &home, &path, &db_path);
}

#[cfg(unix)]
fn assert_installed_native_diff(
    installed: &std::path::Path,
    home: &std::path::Path,
    path: &str,
    db_path: &std::path::Path,
    diff_argv: &std::path::Path,
) {
    let native_diff = Command::new(installed)
        .args(["--", "diff", "-qr", "left", "right"])
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_INSIGHTS_DB_PATH", db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .env("CX_DIFF_ARGV_PATH", diff_argv)
        .output()
        .unwrap();
    assert!(native_diff.status.success(), "{native_diff:?}");
    assert_eq!(
        String::from_utf8_lossy(&native_diff.stdout),
        "native-diff-executed\n"
    );
    assert_eq!(fs::read_to_string(diff_argv).unwrap(), "-qr\nleft\nright\n");

    let official_diff = Command::new(installed)
        .args(["diff", "--stat"])
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_DISABLE_INSIGHTS", "1")
        .output()
        .unwrap();
    assert!(official_diff.status.success(), "{official_diff:?}");
    assert_eq!(
        String::from_utf8_lossy(&official_diff.stdout),
        "git-diff-stat-executed\n"
    );
}

#[cfg(unix)]
fn assert_installed_native_find(
    installed: &std::path::Path,
    home: &std::path::Path,
    path: &str,
    db_path: &std::path::Path,
    find_argv: &std::path::Path,
) {
    let native_find = Command::new(installed)
        .args([
            "--", "find", "build", "-type", "f", "(", "-name", "one", "-o", "-name", "two", ")",
            "-print",
        ])
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_INSIGHTS_DB_PATH", db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .env("CX_FIND_ARGV_PATH", find_argv)
        .output()
        .unwrap();
    assert!(native_find.status.success(), "{native_find:?}");
    assert_eq!(
        String::from_utf8_lossy(&native_find.stdout),
        "build/image-service\nbuild/identity-service\n"
    );
    assert_eq!(
        fs::read_to_string(find_argv).unwrap(),
        "build\n-type\nf\n(\n-name\none\n-o\n-name\ntwo\n)\n-print\n"
    );
}

#[cfg(unix)]
fn assert_installed_native_rg(
    installed: &std::path::Path,
    home: &std::path::Path,
    path: &str,
    db_path: &std::path::Path,
    rg_argv: &std::path::Path,
) {
    let pattern = ".{0,220}translateY\\(50px\\).{0,260}";
    let native_rg = Command::new(installed)
        .args(["--", "rg", "-o", "--pcre2", pattern, "steamui.css"])
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_INSIGHTS_DB_PATH", db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .env("CX_RG_ARGV_PATH", rg_argv)
        .output()
        .unwrap();
    assert!(native_rg.status.success(), "{native_rg:?}");
    assert_eq!(
        String::from_utf8_lossy(&native_rg.stdout),
        "transform:translateY(50px)\n"
    );
    assert_eq!(
        fs::read_to_string(rg_argv).unwrap(),
        format!("-o\n--pcre2\n{pattern}\nsteamui.css\n")
    );

    let pattern = "\"preview/[A-Za-z0-9_./-]+\"";
    let native_rg = Command::new(installed)
        .args([
            "--",
            "rg",
            "-o",
            "--no-filename",
            pattern,
            "packages/intelligence-ui/src/chat",
        ])
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_INSIGHTS_DB_PATH", db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .env("CX_RG_ARGV_PATH", rg_argv)
        .output()
        .unwrap();
    assert!(native_rg.status.success(), "{native_rg:?}");
    assert_eq!(
        fs::read_to_string(rg_argv).unwrap(),
        format!("-o\n--no-filename\n{pattern}\npackages/intelligence-ui/src/chat\n")
    );
}

#[cfg(unix)]
fn assert_installed_silent_failure(
    installed: &std::path::Path,
    home: &std::path::Path,
    path: &str,
    db_path: &std::path::Path,
) {
    let silent = Command::new(installed)
        .args(["--", "silent-fail"])
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_INSIGHTS_DB_PATH", db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert_eq!(silent.status.code(), Some(1), "{silent:?}");
    assert!(silent.stdout.is_empty(), "{silent:?}");
    assert!(silent.stderr.is_empty(), "{silent:?}");
    assert!(!home.join(".cx/cache/failures/passthrough").exists());

    let connection = rusqlite::Connection::open(db_path).unwrap();
    let artifact_count: u64 = connection
        .query_row("SELECT COUNT(*) FROM failure_artifacts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(artifact_count, 0);
    let failure: (u64, u64, String) = connection
        .query_row(
            "SELECT length(cx_response), length(raw_response), artifact_ref \
             FROM command_failures WHERE command_family = 'passthrough silent-fail' \
             ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(failure, (0, 0, String::new()));
}

fn structured_source_line() -> String {
    concat!(
        "r#\"{{\"rawBytes\":{raw_bytes},\"rawChars\":{raw_chars},",
        "\"rawLines\":{raw_lines},\"rawTokens\":{raw_tokens},",
        "\"emittedBytes\":{emitted_bytes},\"emittedChars\":{emitted_chars},",
        "\"emittedLines\":{emitted_lines},\"emittedTokens\":{emitted_tokens},",
        "\"savedBytes\":{saved_bytes},\"savedChars\":{saved_chars},",
        "\"savedLines\":{saved_lines},\"savedTokens\":{saved_tokens}}}\"#,"
    )
    .to_string()
}

#[cfg(unix)]
#[test]
fn installed_wrapper_runs_documented_insights_presentation_contract() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    let db_path = temp.path().join("insights.sqlite");
    let fixture = temp.path().join("documented-example.rs");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        &fixture,
        (0..120)
            .map(|line| format!("pub const DOCUMENTED_{line:03}: usize = {line};\n"))
            .collect::<String>(),
    )
    .unwrap();

    let install = Command::new("sh")
        .arg(repo_root().join("scripts/install.sh"))
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", env!("CARGO_BIN_EXE_cx"))
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let installed = home.join(".local/bin/cx");
    let settings = run_installed(
        &installed,
        &home,
        &db_path,
        &["insights", "settings", "--set", "record_invocations=true"],
    );
    assert!(settings.status.success(), "{settings:?}");

    let read = run_installed(
        &installed,
        &home,
        &db_path,
        &["read", fixture.to_str().unwrap(), "--head", "4"],
    );
    assert!(read.status.success(), "{read:?}");
    assert!(String::from_utf8_lossy(&read.stdout).contains("DOCUMENTED_000"));

    let expanded_read = run_installed(
        &installed,
        &home,
        &db_path,
        &[
            "read",
            fixture.to_str().unwrap(),
            "--range",
            "1:1",
            "--line-numbers",
        ],
    );
    assert!(expanded_read.status.success(), "{expanded_read:?}");

    let rejected = run_installed(
        &installed,
        &home,
        &db_path,
        &[
            "--",
            "read",
            "--head",
            "1",
            "--tail",
            "1",
            fixture.to_str().unwrap(),
        ],
    );
    assert_eq!(rejected.status.code(), Some(2));

    assert_installed_presentation_contract(&installed, &home, &db_path);
    assert_installed_node_trailing_check_contract(
        &installed,
        &home,
        &db_path.with_file_name("node-routing.sqlite"),
        &bin,
    );
    assert_installed_report_lifecycle_contract(&installed, &home, &db_path);
    assert_installed_report_triage_contract(&installed, &home, &db_path);
    assert_installed_export_contract(&installed, &home, &db_path);
    assert_installed_dashboard_contract(&installed, &home, &db_path);
    assert_installed_routing_contract(&installed, &home, &db_path);
    assert_installed_failure_coverage_contract(&installed, &home, &db_path, &bin);
}

#[cfg(unix)]
fn assert_installed_node_trailing_check_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
    bin: &std::path::Path,
) {
    let node = bin.join("node");
    fs::write(&node, "#!/bin/sh\nprintf 'runtime argv: %s\\n' \"$*\"\n").unwrap();
    make_executable(&node);
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let setting = run_installed_with_path(
        installed,
        home,
        db_path,
        &path,
        &[
            "insights",
            "settings",
            "--set",
            "passthrough_unsupported_commands=true",
        ],
    );
    assert!(setting.status.success(), "{setting:?}");

    let runtime = run_installed_with_path(
        installed,
        home,
        db_path,
        &path,
        &["--", "node", "script.mjs", "--check"],
    );
    assert!(runtime.status.success(), "{runtime:?}");
    assert_eq!(
        String::from_utf8_lossy(&runtime.stdout),
        "runtime argv: script.mjs --check\n"
    );

    let syntax = run_installed_with_path(
        installed,
        home,
        db_path,
        &path,
        &["--", "node", "--check", "script.mjs"],
    );
    assert!(syntax.status.success(), "{syntax:?}");
    assert!(
        String::from_utf8_lossy(&syntax.stdout).contains("node --check: syntax ok (script.mjs)")
    );
}

#[cfg(unix)]
fn assert_installed_report_lifecycle_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
) {
    let report = run_installed(
        installed,
        home,
        db_path,
        &["report", "cx", "grep", "route|path", "src"],
    );
    assert!(report.status.success(), "{report:?}");
    assert!(String::from_utf8_lossy(&report.stdout).contains("Status: open"));

    let update = run_installed(
        installed,
        home,
        db_path,
        &[
            "insights",
            "report-update",
            "1",
            "--status",
            "native-parity",
            "--note",
            "native command produced the same result",
            "--revision",
            "r111",
        ],
    );
    assert!(update.status.success(), "{update:?}");
    assert!(String::from_utf8_lossy(&update.stdout).contains("Status: native_parity"));

    let reports = run_installed(
        installed,
        home,
        db_path,
        &[
            "insights",
            "reports",
            "--status",
            "native-parity",
            "--limit",
            "3",
        ],
    );
    assert!(reports.status.success(), "{reports:?}");
    let stdout = String::from_utf8_lossy(&reports.stdout);
    assert!(stdout.contains("Lifecycle: 1 total | 0 open | 0 resolved | 1 native parity"));
    assert!(stdout.contains("native_parity | - | - | no-match | suspicious_output"));
}

#[cfg(unix)]
fn assert_installed_report_triage_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
) {
    for args in [
        &["report", "git", "status", "--short"][..],
        &["report", "git", "status", "--short"][..],
        &["report", "sh"][..],
    ] {
        let report = run_installed(installed, home, db_path, args);
        assert!(report.status.success(), "{report:?}");
    }

    let preview = run_installed(
        installed,
        home,
        db_path,
        &[
            "insights",
            "report-triage",
            "--format",
            "json",
            "--limit",
            "1",
        ],
    );
    assert!(preview.status.success(), "{preview:?}");
    let preview_json: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_installed_triage_preview_json(&preview_json);

    let applied = run_installed(
        installed,
        home,
        db_path,
        &[
            "insights",
            "report-triage",
            "--apply",
            "--format",
            "json",
            "--limit",
            "10",
        ],
    );
    assert!(applied.status.success(), "{applied:?}");
    let applied_json: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_installed_triage_applied_json(&applied_json);

    let denied = run_installed(
        installed,
        home,
        db_path,
        &["insights", "reports", "--status", "denied", "--limit", "10"],
    );
    assert!(denied.status.success(), "{denied:?}");
    let denied_stdout = String::from_utf8_lossy(&denied.stdout);
    assert!(denied_stdout.contains("Status filter: denied"));
    assert!(denied_stdout.contains("duplicate"));
    assert!(denied_stdout.contains("insufficient_evidence"));
    assert!(denied_stdout.contains("invalid"));

    let second_preview = run_installed(
        installed,
        home,
        db_path,
        &["insights", "report-triage", "--format", "json"],
    );
    assert!(second_preview.status.success(), "{second_preview:?}");
    let second_json: serde_json::Value = serde_json::from_slice(&second_preview.stdout).unwrap();
    assert_eq!(second_json["proposal_count"], 0);

    let connection = rusqlite::Connection::open(db_path).unwrap();
    let schema_version: String = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'insights_schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_version, "20");
}

#[cfg(unix)]
fn assert_installed_triage_preview_json(preview_json: &serde_json::Value) {
    assert_eq!(preview_json["schema_version"], 2);
    assert_eq!(preview_json["mode"], "dry-run");
    assert_eq!(preview_json["proposal_count"], 3);
    assert_eq!(preview_json["proposal_summary"]["duplicate"], 1);
    assert_eq!(preview_json["proposal_summary"]["invalid"], 1);
    assert_eq!(preview_json["proposal_summary"]["insufficient_evidence"], 1);
    assert_eq!(preview_json["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(preview_json["proposal_rows_truncated"], 2);
    assert_eq!(
        preview_json["policy"]["invocation_linked_reports_protected"],
        true
    );
    assert_eq!(
        preview_json["policy"]["artifact_linked_reports_protected"],
        true
    );
}

#[cfg(unix)]
fn assert_installed_triage_applied_json(applied_json: &serde_json::Value) {
    assert_eq!(applied_json["mode"], "applied");
    assert_eq!(applied_json["applied_count"], 3);
    assert_eq!(applied_json["final_lifecycle"]["denied"], 3);
}

#[cfg(unix)]
fn assert_installed_presentation_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
) {
    let presentation = run_installed(
        installed,
        home,
        db_path,
        &["insights", "presentation", "--limit", "3"],
    );
    assert!(presentation.status.success(), "{presentation:?}");
    let presentation_stdout = String::from_utf8(presentation.stdout).unwrap();
    assert!(presentation_stdout.contains("Metric scorecard:"));
    assert!(presentation_stdout.contains("Saved output:"));
    assert!(presentation_stdout.contains("Context-window equivalents saved:"));
    assert!(presentation_stdout.contains("Savings distribution:"));
    assert!(presentation_stdout.contains("Saved excluding top 10 invocations:"));
}

#[cfg(unix)]
fn assert_installed_export_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
) {
    let export = run_installed(
        installed,
        home,
        db_path,
        &["insights", "export", "--format", "json", "--limit", "3"],
    );
    assert!(export.status.success(), "{export:?}");
    let value: serde_json::Value = serde_json::from_slice(&export.stdout).unwrap();
    assert_eq!(value["schema_name"], "cx-insights-export");
    assert_eq!(value["schema_version"], 18);
    assert_eq!(value["presentation"]["metrics"]["invocations"], 2);
    assert_eq!(value["presentation"]["metrics"]["expansions"], 1);
    assert!(
        value["presentation"]["metrics"]["saved"]["tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        value["recent_invocations"][0]["expansion_reason"],
        "read-formatting"
    );
    assert!(value["recent_invocations"][0]["raw"]["tokens"].is_u64());
    assert!(value["recent_invocations"][0]["emitted"]["tokens"].is_u64());
    assert_eq!(value["savings_distribution"]["invocations"], 2);
    assert!(value["savings_distribution"]["concentration"]["top_10_share"].is_f64());
    assert_eq!(value["routing_summary"]["rejections"], 1);
    assert_eq!(value["command_report_status"]["denied"], 3);
    assert_eq!(value["command_report_denial_reasons"]["duplicate"], 1);
    assert_eq!(
        value["command_report_denial_reasons"]["insufficient_evidence"],
        1
    );
    assert_eq!(value["command_report_denial_reasons"]["invalid"], 1);
}

#[cfg(unix)]
fn assert_installed_dashboard_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
) {
    let dashboard = run_installed(
        installed,
        home,
        db_path,
        &["insights", "dashboard", "--limit", "3"],
    );
    assert!(dashboard.status.success(), "{dashboard:?}");
    let value: serde_json::Value = serde_json::from_slice(&dashboard.stdout).unwrap();
    assert_eq!(value["schema_name"], "cx-insights-dashboard");
    assert_eq!(value["schema_version"], 12);
    assert_eq!(value["source_export_schema_version"], 18);
    assert_eq!(
        value["contract"]["metric_semantics"]["tokens_are_estimates"],
        true
    );
    assert!(value["tables"]["recent_invocations"][0]["raw"]["tokens"].is_u64());
    assert!(value["tables"]["expanded_invocations"].as_array().is_some());
    assert_eq!(value["savings_distribution"]["invocations"], 2);
    assert_eq!(value["health"]["routing_summary"]["rejections"], 1);
    assert_eq!(value["health"]["denied_quality_reports"], 3);
    assert_eq!(
        value["health"]["quality_report_denial_reasons"]["duplicate"],
        1
    );
}

#[cfg(unix)]
fn assert_installed_routing_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
) {
    let routing = run_installed(
        installed,
        home,
        db_path,
        &["insights", "routing", "--limit", "3"],
    );
    assert!(routing.status.success(), "{routing:?}");
    let stdout = String::from_utf8(routing.stdout).unwrap();
    assert!(stdout.contains("Rejected: 1"));
    assert!(stdout.contains("cx-owned-parse-error"));
}

#[cfg(unix)]
fn assert_installed_failure_coverage_contract(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
    bin: &std::path::Path,
) {
    for name in ["alpha-fail", "beta-fail"] {
        let path = bin.join(name);
        fs::write(&path, "#!/bin/sh\nexit 7\n").unwrap();
        make_executable(&path);
    }
    let gamma = bin.join("gamma-fail");
    fs::write(&gamma, "#!/bin/sh\nprintf 'gamma failure\\n' >&2\nexit 9\n").unwrap();
    make_executable(&gamma);
    let delta = bin.join("delta-fail");
    fs::write(&delta, "#!/bin/sh\nexit 9\n").unwrap();
    make_executable(&delta);
    let path = format!("{}:/usr/bin:/bin", bin.display());

    for setting in [
        "passthrough_unsupported_commands=true",
        "record_failures=false",
    ] {
        let output = run_installed_with_path(
            installed,
            home,
            db_path,
            &path,
            &["insights", "settings", "--set", setting],
        );
        assert!(output.status.success(), "{output:?}");
    }
    for command in ["alpha-fail", "beta-fail"] {
        let output = run_installed_with_path(installed, home, db_path, &path, &["--", command]);
        assert_eq!(output.status.code(), Some(7), "{output:?}");
    }
    for setting in ["record_failures=true", "record_failure_responses=true"] {
        let output = run_installed_with_path(
            installed,
            home,
            db_path,
            &path,
            &["insights", "settings", "--set", setting],
        );
        assert!(output.status.success(), "{output:?}");
    }
    let gamma_output =
        run_installed_with_path(installed, home, db_path, &path, &["--", "gamma-fail"]);
    assert_eq!(gamma_output.status.code(), Some(9), "{gamma_output:?}");
    let delta_output =
        run_installed_with_path(installed, home, db_path, &path, &["--", "delta-fail"]);
    assert_eq!(delta_output.status.code(), Some(9), "{delta_output:?}");

    let limited = run_installed_with_path(
        installed,
        home,
        db_path,
        &path,
        &["insights", "audit", "--format", "json", "--limit", "1"],
    );
    let complete = run_installed_with_path(
        installed,
        home,
        db_path,
        &path,
        &["insights", "audit", "--format", "json", "--limit", "100"],
    );
    assert!(limited.status.success(), "{limited:?}");
    assert!(complete.status.success(), "{complete:?}");
    let limited_json: serde_json::Value = serde_json::from_slice(&limited.stdout).unwrap();
    let complete_json: serde_json::Value = serde_json::from_slice(&complete.stdout).unwrap();
    assert_eq!(limited_json["schema_version"], 4);
    assert_eq!(
        limited_json["summary"]["artifact_coverage"],
        complete_json["summary"]["artifact_coverage"]
    );
    assert_eq!(
        limited_json["summary"]["artifact_coverage"]["unknown_invocations"], 2,
        "{}",
        limited_json["summary"]["artifact_coverage"]
    );
    assert_eq!(
        limited_json["summary"]["artifact_coverage"]["silent_detail_rows"],
        1
    );
    assert_eq!(
        limited_json["summary"]["artifact_coverage"]["artifact_linked_detail_rows"],
        1
    );
    assert_eq!(
        limited_json["summary"]["artifact_coverage"]["output_gap_detail_rows"],
        0
    );
}

#[cfg(unix)]
fn run_installed(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
    Command::new(installed)
        .args(args)
        .current_dir(repo_root())
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("CX_INSIGHTS_DB_PATH", db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap()
}

#[cfg(unix)]
fn run_installed_with_path(
    installed: &std::path::Path,
    home: &std::path::Path,
    db_path: &std::path::Path,
    path: &str,
    args: &[&str],
) -> std::process::Output {
    Command::new(installed)
        .args(args)
        .current_dir(repo_root())
        .env("HOME", home)
        .env("PATH", path)
        .env("CX_INSIGHTS_DB_PATH", db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap()
}
