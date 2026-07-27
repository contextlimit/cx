#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cx_command(home: &Path, bin: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
    command
        .env("HOME", home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
    command
}

fn write_executable(bin: &Path, name: &str, body: &str) {
    fs::create_dir_all(bin).unwrap();
    let path = bin.join(name);
    fs::write(&path, body).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn fixture_roots(label: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(format!("{label}-home"));
    let bin = temp.path().join(format!("{label}-bin"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    (temp, home, bin)
}

#[test]
fn silent_passthrough_failure_records_failure_without_an_empty_artifact() {
    let (temp, home, bin) = fixture_roots("silent-failure");
    let db_path = temp.path().join("silent-failure.sqlite");
    write_executable(&bin, "silent-fail", "#!/bin/sh\nexit 1\n");
    write_executable(&bin, "stdout-fail", "#!/bin/sh\nprintf x\nexit 1\n");

    let settings = cx_command(&home, &bin)
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
        .env("CX_INSIGHTS_DB_PATH", &db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert!(settings.status.success(), "{settings:?}");

    let silent = cx_command(&home, &bin)
        .args(["--", "silent-fail"])
        .env("CX_INSIGHTS_DB_PATH", &db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert_eq!(silent.status.code(), Some(1), "{silent:?}");
    assert!(silent.stdout.is_empty(), "{silent:?}");
    assert!(silent.stderr.is_empty(), "{silent:?}");
    assert!(!home.join(".cx/cache/failures/passthrough").exists());

    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let invocation: (u64, u64, u64, String) = connection
        .query_row(
            "SELECT raw_bytes, emitted_bytes, expanded_bytes, expansion_reason \
             FROM command_invocations WHERE command_family = 'passthrough silent-fail' \
             ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(invocation, (0, 0, 0, String::new()));

    let failure: (i32, u64, u64, String) = connection
        .query_row(
            "SELECT exit_code, length(cx_response), length(raw_response), artifact_ref \
             FROM command_failures WHERE command_family = 'passthrough silent-fail' \
             ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(failure, (1, 0, 0, String::new()));
    let artifact_count: u64 = connection
        .query_row("SELECT COUNT(*) FROM failure_artifacts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(artifact_count, 0);
    drop(connection);

    let nonempty = cx_command(&home, &bin)
        .args(["--", "stdout-fail"])
        .env("CX_INSIGHTS_DB_PATH", &db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert_eq!(nonempty.status.code(), Some(1), "{nonempty:?}");
    assert!(
        String::from_utf8_lossy(&nonempty.stdout)
            .contains("[full output: ~/.cx/cache/failures/passthrough/"),
        "{nonempty:?}"
    );
    assert_eq!(
        fs::read_dir(home.join(".cx/cache/failures/passthrough"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn explicit_auto_native_diff_executes_diff_instead_of_git() {
    let (temp, home, bin) = fixture_roots("native-diff");
    let argv_path = temp.path().join("diff-argv.txt");
    let db_path = temp.path().join("diff-routing.sqlite");
    write_executable(
        &bin,
        "diff",
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_TEST_ARGV_PATH\"\nprintf 'native-diff-executed\\n'\n",
    );
    write_executable(
        &bin,
        "git",
        "#!/bin/sh\nif [ \"$1\" = diff ] && [ \"$2\" = --stat ]; then\nprintf 'git-diff-stat-executed\\n'\nexit 0\nfi\nprintf 'unexpected-git: %s\\n' \"$*\" >&2\nexit 97\n",
    );

    let output = cx_command(&home, &bin)
        .args(["--", "diff", "-qr", "left", "right"])
        .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
        .env("CX_TEST_ARGV_PATH", &argv_path)
        .env("CX_INSIGHTS_DB_PATH", &db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "native-diff-executed\n"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("unexpected-git"));
    assert_eq!(
        fs::read_to_string(&argv_path).unwrap(),
        "-qr\nleft\nright\n"
    );

    let official = cx_command(&home, &bin)
        .args(["diff", "--stat"])
        .env("CX_DISABLE_INSIGHTS", "1")
        .output()
        .unwrap();
    assert!(official.status.success(), "{official:?}");
    assert_eq!(
        String::from_utf8_lossy(&official.stdout),
        "git-diff-stat-executed\n"
    );

    let connection = rusqlite::Connection::open(db_path).unwrap();
    let row: (String, String, String, String) = connection
        .query_row(
            "SELECT process, command_family, command, argv_json \
             FROM command_invocations ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(row.0, "diff");
    assert_eq!(row.1, "diff");
    assert_eq!(row.2, "diff -qr left right");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&row.3).unwrap(),
        ["cx", "--", "diff", "-qr", "left", "right"]
    );
}

#[test]
fn default_passthrough_records_invocation_without_setup() {
    let (_temp, home, bin) = fixture_roots("default-passthrough");
    write_executable(&bin, "audit-tool", "#!/bin/sh\nprintf 'audit-ok\\n'\n");

    let output = cx_command(&home, &bin)
        .args(["--", "audit-tool"])
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "audit-ok\n");
    assert!(output.stderr.is_empty(), "{output:?}");
    let connection = rusqlite::Connection::open(home.join(".cx/db.sqlite")).unwrap();
    let row = connection
        .query_row(
            "SELECT process, command_family, command, argv_json, command_shape \
             FROM command_invocations ORDER BY id DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "audit-tool");
    assert_eq!(row.1, "passthrough audit-tool");
    assert_eq!(row.2, "passthrough audit-tool");
    assert_eq!(row.3, "[]");
    assert_eq!(row.4, "audit-tool");
}

#[test]
fn default_passthrough_respects_disabled_insights_without_database_creation() {
    let (_temp, home, bin) = fixture_roots("disabled-insights-passthrough");
    write_executable(&bin, "audit-tool", "#!/bin/sh\nprintf 'audit-ok\\n'\n");

    let output = cx_command(&home, &bin)
        .args(["--", "audit-tool"])
        .env("CX_DISABLE_INSIGHTS", "1")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "audit-ok\n");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(!home.join(".cx/db.sqlite").exists());
}

#[test]
fn explicit_auto_find_boolean_expression_executes_native_find() {
    let (temp, home, bin) = fixture_roots("native-find");
    let argv_path = temp.path().join("find-argv.txt");
    let db_path = temp.path().join("find-routing.sqlite");
    write_executable(
        &bin,
        "find",
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_TEST_ARGV_PATH\"\nprintf 'build/image-service\\nbuild/identity-service\\n'\n",
    );

    let output = cx_command(&home, &bin)
        .args([
            "--",
            "find",
            "build",
            "-path",
            "*/build*/*",
            "-type",
            "f",
            "(",
            "-name",
            "identity-service",
            "-o",
            "-name",
            "image-service",
            ")",
            "-print",
        ])
        .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
        .env("CX_TEST_ARGV_PATH", &argv_path)
        .env("CX_INSIGHTS_DB_PATH", &db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "build/image-service\nbuild/identity-service\n"
    );
    assert_eq!(
        fs::read_to_string(&argv_path).unwrap(),
        concat!(
            "build\n",
            "-path\n",
            "*/build*/*\n",
            "-type\n",
            "f\n",
            "(\n",
            "-name\n",
            "identity-service\n",
            "-o\n",
            "-name\n",
            "image-service\n",
            ")\n",
            "-print\n"
        )
    );

    let simple_root = temp.path().join("simple-find");
    fs::create_dir_all(&simple_root).unwrap();
    fs::write(simple_root.join("fixture.rs"), "fn fixture() {}\n").unwrap();
    let simple_root_text = simple_root.to_string_lossy().to_string();
    let simple = cx_command(&home, &bin)
        .args([
            "--",
            "find",
            simple_root_text.as_str(),
            "-type",
            "f",
            "-name",
            "*.rs",
        ])
        .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
        .env("CX_INSIGHTS_DB_PATH", &db_path)
        .env_remove("CX_DISABLE_INSIGHTS")
        .output()
        .unwrap();
    assert!(simple.status.success(), "{simple:?}");
    let simple_stdout = String::from_utf8_lossy(&simple.stdout);
    assert!(simple_stdout.contains("fixture.rs"), "{simple_stdout}");
    assert!(simple_stdout.contains("find: 1 entries"), "{simple_stdout}");
}
