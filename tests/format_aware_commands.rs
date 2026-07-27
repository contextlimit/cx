#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::{tempdir, TempDir};

struct Fixture {
    _temp: TempDir,
    bin: PathBuf,
    home: PathBuf,
    db: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempdir().unwrap();
        let bin = temp.path().join("bin");
        let home = temp.path().join("home");
        let db = temp.path().join("insights.sqlite");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        Self {
            _temp: temp,
            bin,
            home,
            db,
        }
    }

    fn tool(&self, name: &str, body: &str) -> PathBuf {
        let path = self.bin.join(name);
        fs::write(&path, body).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cx"))
            .args(args)
            .env("PATH", &self.bin)
            .env("HOME", &self.home)
            .env("CX_INSIGHTS_DB_PATH", &self.db)
            .env_remove("CX_DISABLE_INSIGHTS")
            .env("CX_ENABLE_UNSUPPORTED_PASSTHROUGH", "1")
            .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1")
            .env_remove("CX_TOOL_FALLBACK_PATHS")
            .output()
            .unwrap()
    }
}

#[test]
fn native_and_cx_node_test_shapes_share_tap_compaction() {
    let fixture = Fixture::new();
    fixture.tool("node", &long_tap_script(0));

    let native = fixture.run(&["--", "node", "--test", "suite.test.mjs"]);
    assert_success(&native);
    let native_stdout = String::from_utf8_lossy(&native.stdout);
    assert!(native_stdout.contains("node test: ok"));
    assert!(native_stdout.contains("# tests 100"));
    assert!(native_stdout.contains("lines omitted"));
    assert!(!native_stdout.contains("ok 50 - test 50"));

    let cx_test = fixture.run(&["node", "test", "suite.test.mjs"]);
    assert_success(&cx_test);
    let cx_stdout = String::from_utf8_lossy(&cx_test.stdout);
    assert!(cx_stdout.contains("node test: ok"));
    assert!(cx_stdout.contains("# pass 100"));
    assert!(cx_stdout.contains("lines omitted"));
}

#[test]
fn npm_npx_and_dotnet_test_build_shapes_compact_known_formats() {
    let fixture = Fixture::new();
    fixture.tool("npm", &npm_script());
    fixture.tool("npx", &npx_script());
    fixture.tool("dotnet", &dotnet_script());

    let npm_test = fixture.run(&["--", "npm", "test"]);
    assert_success(&npm_test);
    assert_output_contains(&npm_test, &["npm test: ok", "# tests 100", "lines omitted"]);

    let npm_build = fixture.run(&["--", "npm", "run", "build"]);
    assert_success(&npm_build);
    assert_output_contains(
        &npm_build,
        &["npm build: ok", "100 modules transformed", "built in 500ms"],
    );

    let npx_test = fixture.run(&["--", "npx", "vitest", "run"]);
    assert_success(&npx_test);
    assert_output_contains(&npx_test, &["npx test: ok", "Tests 100 passed"]);

    let npx_build = fixture.run(&["--", "npx", "vite", "build"]);
    assert_success(&npx_build);
    assert_output_contains(&npx_build, &["npx build: ok", "built in 400ms"]);

    let dotnet_test = fixture.run(&["--", "dotnet", "test", "service.sln"]);
    assert_success(&dotnet_test);
    assert_output_contains(
        &dotnet_test,
        &["dotnet test: ok", "Passed!", "Total tests: 100"],
    );

    let dotnet_build = fixture.run(&["--", "dotnet", "build", "service.sln"]);
    assert_success(&dotnet_build);
    assert_output_contains(
        &dotnet_build,
        &["dotnet build: ok", "Build succeeded.", "0 Error(s)"],
    );
}

#[test]
fn clang_format_and_cmake_shapes_compact_diagnostics_and_progress() {
    let fixture = Fixture::new();
    fixture.tool("clang-format", &clang_format_script());
    let cmake = fixture.tool("cmake", &cmake_script());

    let clang = fixture.run(&["--", "clang-format", "--dry-run", "src/main.cpp"]);
    assert_success(&clang);
    assert_output_contains(
        &clang,
        &[
            "clang-format diagnostics: ok",
            "src/file1.cpp",
            "src/file100.cpp",
        ],
    );

    let configure = fixture.run(&["--", "cmake", "-S", ".", "-B", "build"]);
    assert_success(&configure);
    assert_output_contains(
        &configure,
        &[
            "cmake configure: ok",
            "-- Configuring done",
            "-- Build files have been written to:",
        ],
    );

    let cmake_path = cmake.to_string_lossy().to_string();
    let passthrough_build = fixture.run(&["--", &cmake_path, "--build", "build"]);
    assert_success(&passthrough_build);
    assert_output_contains(
        &passthrough_build,
        &["cmake build: ok", "Built target product", "lines omitted"],
    );

    let official_build = fixture.run(&["--", "cmake", "--build", "build"]);
    assert_success(&official_build);
    assert_output_contains(
        &official_build,
        &["cmake build: ok", "Built target product"],
    );
}

#[test]
fn arbitrary_node_jq_and_curl_output_remains_exact() {
    let fixture = Fixture::new();
    let script = exact_output_script();
    fixture.tool("node", &script);
    fixture.tool("jq", &script);
    fixture.tool("curl", &script);
    let expected = (1..=100)
        .map(|index| format!("raw_{index:03}\n"))
        .collect::<String>();

    for args in [
        vec!["--", "node", "script.mjs"],
        vec!["--", "jq", ".", "data.json"],
        vec!["--", "curl", "https://example.test/data"],
    ] {
        let output = fixture.run(&args);
        assert_success(&output);
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn command_optimizations_setting_disables_format_compaction() {
    let fixture = Fixture::new();
    fixture.tool("node", &long_tap_script(0));

    let settings = fixture.run(&[
        "insights",
        "settings",
        "--set",
        "command_optimizations=false",
    ]);
    assert_success(&settings);

    let output = fixture.run(&["node", "test", "suite.test.mjs"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TAP version 13"));
    assert!(stdout.contains("ok 50 - test 50"));
    assert!(stdout.contains("# tests 100"));
    assert!(!stdout.contains("node test: ok"));
    assert!(!stdout.contains("lines omitted"));
}

#[test]
fn sqlite3_self_database_queries_receive_a_busy_timeout_rewrite() {
    let fixture = Fixture::new();
    let args_file = fixture._temp.path().join("sqlite3-args.txt");
    fixture.tool(
        "sqlite3",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '42\\n'\n",
            args_file.display()
        ),
    );

    let output = fixture.run(&[
        "--",
        "sqlite3",
        fixture.db.to_str().unwrap(),
        "SELECT COUNT(*) FROM command_invocations;",
    ]);
    assert_success(&output);
    assert_eq!(output.stdout, b"42\n");

    let args = fs::read_to_string(&args_file).unwrap();
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        vec![
            "-cmd",
            ".timeout 5000",
            fixture.db.to_str().unwrap(),
            "SELECT COUNT(*) FROM command_invocations;"
        ]
    );
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    let repair: (String, String) = connection
        .query_row(
            "SELECT rule_id, action FROM command_repairs ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(repair.0, "sqlite3.cx_database_busy_timeout");
    assert_eq!(repair.1, "preflight_rewrite");
}

#[test]
fn command_optimizations_setting_disables_sqlite3_busy_timeout_rewrite() {
    let fixture = Fixture::new();
    let args_file = fixture._temp.path().join("sqlite3-unmodified-args.txt");
    fixture.tool(
        "sqlite3",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '42\\n'\n",
            args_file.display()
        ),
    );
    let settings = fixture.run(&[
        "insights",
        "settings",
        "--set",
        "command_optimizations=false",
    ]);
    assert_success(&settings);

    let output = fixture.run(&["--", "sqlite3", fixture.db.to_str().unwrap(), "SELECT 1;"]);
    assert_success(&output);
    assert_eq!(
        fs::read_to_string(args_file)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec![fixture.db.to_str().unwrap(), "SELECT 1;"]
    );
}

#[test]
fn compacted_failure_retains_exit_code_evidence_and_full_artifact() {
    let fixture = Fixture::new();
    fixture.tool("node", &long_tap_script(1));

    let output = fixture.run(&["--", "node", "--test", "suite.test.mjs"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("node test: failed (exit 1)"));
    assert!(stdout.contains("not ok 57 - contract failure"));
    assert!(stdout.contains("expected true but received false"));
    assert!(stdout.contains("[full output: ~/.cx/cache/failures/passthrough/"));
    assert!(!stdout.contains("ok 40 - test 40"));

    let artifact = first_file(&fixture.home.join(".cx/cache/failures/passthrough"));
    let artifact_body = fs::read_to_string(artifact).unwrap();
    assert!(artifact_body.contains("ok 40 - test 40"));
    assert!(artifact_body.contains("not ok 57 - contract failure"));
}

fn long_tap_script(exit_code: i32) -> String {
    format!(
        "#!/bin/sh\nprintf 'TAP version 13\\n'\ni=1\nwhile [ \"$i\" -le 100 ]; do\n  if [ {exit_code} -ne 0 ] && [ \"$i\" -eq 57 ]; then\n    printf 'not ok 57 - contract failure\\n  error: expected true but received false\\n'\n  else\n    printf 'ok %s - test %s\\n' \"$i\" \"$i\"\n  fi\n  i=$((i + 1))\ndone\nprintf '1..100\\n# tests 100\\n# pass %s\\n# fail %s\\n# duration_ms 20\\n' \"$((100 - {exit_code}))\" \"${{exit_code}}\"\nexit {exit_code}\n"
    )
}

fn npm_script() -> String {
    r#"#!/bin/sh
if [ "$1" = "test" ]; then
  printf '> fixture@1.0.0 test\n> node --test\nTAP version 13\n'
  i=1; while [ "$i" -le 100 ]; do printf 'ok %s - npm test %s\n' "$i" "$i"; i=$((i + 1)); done
  printf '1..100\n# tests 100\n# pass 100\n# fail 0\n'
  exit 0
fi
printf '> fixture@1.0.0 build\n> vite build\nvite v6 building for production...\n'
i=1; while [ "$i" -le 100 ]; do printf 'transforming module %s\n' "$i"; i=$((i + 1)); done
printf '100 modules transformed.\nbuilt in 500ms\n'
"#
    .to_string()
}

fn npx_script() -> String {
    r#"#!/bin/sh
if [ "$1" = "vitest" ]; then
  i=1; while [ "$i" -le 100 ]; do printf 'PASS test_%s.mjs\n' "$i"; i=$((i + 1)); done
  printf 'Test Files 100 passed\nTests 100 passed\n'
  exit 0
fi
printf 'vite v6 building for production...\n'
i=1; while [ "$i" -le 100 ]; do printf 'rendering chunk %s\n' "$i"; i=$((i + 1)); done
printf '100 modules transformed.\nbuilt in 400ms\n'
"#
    .to_string()
}

fn dotnet_script() -> String {
    r#"#!/bin/sh
if [ "$1" = "test" ]; then
  i=1; while [ "$i" -le 100 ]; do printf 'Passed test_%s\n' "$i"; i=$((i + 1)); done
  printf 'Passed! - Failed: 0, Passed: 100, Skipped: 0, Total: 100\nTotal tests: 100\n'
  exit 0
fi
i=1; while [ "$i" -le 100 ]; do printf '  Restored project %s\n' "$i"; i=$((i + 1)); done
printf '  service -> /work/bin/service.dll\nBuild succeeded.\n0 Warning(s)\n0 Error(s)\n'
"#
    .to_string()
}

fn clang_format_script() -> String {
    r#"#!/bin/sh
i=1
while [ "$i" -le 100 ]; do
  printf 'src/file%s.cpp:1:1: warning: code should be clang-formatted [-Wclang-format-violations]\n' "$i"
  i=$((i + 1))
done
"#
    .to_string()
}

fn cmake_script() -> String {
    r#"#!/bin/sh
if [ "$1" = "--build" ]; then
  i=1; while [ "$i" -le 100 ]; do printf '[%s/100] Building CXX object file%s.cpp.o\n' "$i" "$i"; i=$((i + 1)); done
  printf 'Built target product\n'
  exit 0
fi
printf '%s\n' '-- The CXX compiler identification is AppleClang 18'
i=1; while [ "$i" -le 100 ]; do printf '%s\n' "-- Detecting CXX compile feature $i"; i=$((i + 1)); done
printf '%s\n' '-- Configuring done' '-- Generating done' '-- Build files have been written to: /work/build'
"#
    .to_string()
}

fn exact_output_script() -> String {
    "#!/bin/sh\ni=1\nwhile [ \"$i\" -le 100 ]; do printf 'raw_%03d\\n' \"$i\"; i=$((i + 1)); done\n"
        .to_string()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_output_contains(output: &Output, needles: &[&str]) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    for needle in needles {
        assert!(stdout.contains(needle), "missing {needle:?} in:\n{stdout}");
    }
}

fn first_file(directory: &Path) -> PathBuf {
    fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
}
