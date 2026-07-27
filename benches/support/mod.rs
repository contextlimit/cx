use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug)]
pub struct ProjectTempDir {
    path: PathBuf,
}

static NEXT_PROJECT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

impl ProjectTempDir {
    pub fn new(label: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".tmp");
        let unique = format!(
            "cx-bench-{label}-{}-{}-{}",
            std::process::id(),
            now_ns(),
            NEXT_PROJECT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        );
        let path = root.join(unique);
        fs::create_dir_all(&path).expect("create project temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProjectTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug)]
pub struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    original_path: Option<OsString>,
    original_disable_fallback: Option<OsString>,
    original_extra_fallback: Option<OsString>,
    original_home: Option<OsString>,
    original_disable_insights: Option<OsString>,
}

impl EnvGuard {
    pub fn fake_path(bin: &Path, home: &Path) -> Self {
        let lock = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let guard = Self {
            _lock: lock,
            original_path: env::var_os("PATH"),
            original_disable_fallback: env::var_os("CX_DISABLE_TOOL_FALLBACK_PATHS"),
            original_extra_fallback: env::var_os("CX_TOOL_FALLBACK_PATHS"),
            original_home: env::var_os("HOME"),
            original_disable_insights: env::var_os("CX_DISABLE_INSIGHTS"),
        };
        env::set_var("PATH", bin);
        env::set_var("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
        env::remove_var("CX_TOOL_FALLBACK_PATHS");
        env::set_var("HOME", home);
        env::remove_var("CX_DISABLE_INSIGHTS");
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        restore_env("PATH", self.original_path.take());
        restore_env(
            "CX_DISABLE_TOOL_FALLBACK_PATHS",
            self.original_disable_fallback.take(),
        );
        restore_env(
            "CX_TOOL_FALLBACK_PATHS",
            self.original_extra_fallback.take(),
        );
        restore_env("HOME", self.original_home.take());
        restore_env("CX_DISABLE_INSIGHTS", self.original_disable_insights.take());
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputMetrics {
    pub raw_bytes: usize,
    pub filtered_bytes: usize,
    pub raw_lines: usize,
    pub filtered_lines: usize,
    pub ratio: f64,
}

impl OutputMetrics {
    pub fn new(raw: &str, filtered: &str) -> Self {
        let raw_bytes = raw.len();
        let filtered_bytes = filtered.len();
        Self {
            raw_bytes,
            filtered_bytes,
            raw_lines: raw.lines().count(),
            filtered_lines: filtered.lines().count(),
            ratio: if raw_bytes == 0 {
                0.0
            } else {
                filtered_bytes as f64 / raw_bytes as f64
            },
        }
    }
}

pub fn assert_reduction(case: &str, raw: &str, filtered: &str, max_ratio: f64, evidence: &[&str]) {
    let metrics = OutputMetrics::new(raw, filtered);
    assert!(
        metrics.filtered_bytes < metrics.raw_bytes,
        "{case}: expected filtered output to be smaller, got {metrics:?}"
    );
    assert!(
        metrics.ratio <= max_ratio,
        "{case}: expected ratio <= {max_ratio}, got {metrics:?}\n{filtered}"
    );
    for expected in evidence {
        assert!(
            filtered.contains(expected),
            "{case}: missing evidence `{expected}` in\n{filtered}"
        );
    }
}

pub fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write fixture file");
}

pub fn write_executable(dir: &Path, name: &str, body: &str) {
    fs::create_dir_all(dir).expect("create bin dir");
    let path = dir.join(name);
    fs::write(&path, body).expect("write executable fixture");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&path).expect("fixture metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("set fixture executable");
    }
}

pub fn output_script(stdout: Option<&Path>, stderr: Option<&Path>, exit_code: i32) -> String {
    let mut script = "#!/bin/sh\n".to_string();
    if let Some(path) = stdout {
        script.push_str(&format!("/bin/cat {}\n", shell_quote(path)));
    }
    if let Some(path) = stderr {
        script.push_str(&format!("/bin/cat {} >&2\n", shell_quote(path)));
    }
    script.push_str(&format!("exit {exit_code}\n"));
    script
}

pub fn shell_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn rust_source_fixture(functions: usize) -> String {
    let mut content = String::new();
    for index in 0..functions {
        content.push_str(&format!("/// Documentation for bench_fn_{index:04}\n"));
        content.push_str("#[inline]\n");
        content.push_str(&format!(
            "pub fn bench_fn_{index:04}(value: usize) -> usize {{\n"
        ));
        content.push_str("    let mut acc = value;\n");
        content.push_str("    acc = acc.wrapping_mul(31).wrapping_add(7);\n");
        content.push_str("    acc\n");
        content.push_str("}\n\n");
    }
    content
}

pub fn grep_output_fixture(matches: usize) -> String {
    let mut output = String::new();
    for index in 0..matches {
        output.push_str(&format!(
            "src/module_{:02}.rs:{}:let needle_{:02} = route_state.update();\n",
            index % 8,
            index + 10,
            index
        ));
    }
    output
}

pub fn file_list_fixture(files: usize) -> String {
    let mut output = String::new();
    for index in 0..files {
        let extension = if index % 2 == 0 { "rs" } else { "toml" };
        output.push_str(&format!("src/module_{index:03}.{extension}\n"));
    }
    output
}

pub fn ls_output_fixture(files: usize) -> String {
    let mut output = String::new();
    output.push_str("total 1024\n");
    output.push_str("drwxr-xr-x  2 user  staff    64 Jan 1 12:00 src\n");
    output.push_str("drwxr-xr-x  2 user  staff    64 Jan 1 12:00 tests\n");
    output.push_str("drwxr-xr-x  2 user  staff    64 Jan 1 12:00 target\n");
    for index in 0..files {
        output.push_str(&format!(
            "-rw-r--r--  1 user  staff  {:4} Jan 1 12:00 file_{index:03}.rs\n",
            100 + index
        ));
    }
    output
}

pub fn docker_ps_output_fixture(containers: usize) -> String {
    let mut output = String::new();
    for index in 0..containers {
        output.push_str(&format!(
            "abcd{index:08}\tweb_{index:03}\tUp {} minutes\tregistry.example.com/app:{index:03}\t0.0.0.0:{}->80/tcp\n",
            index + 1,
            8000 + index
        ));
    }
    output
}

pub fn ps_output_fixture(processes: usize) -> String {
    let mut output = String::from("PID PPID ELAPSED COMMAND\n");
    for index in 0..processes {
        let pid = 1_000 + index;
        let command = if index + 1 == processes {
            "/opt/tools/rare-service --token=sk-secretsecret --serve diagnostics".to_string()
        } else if index % 5 == 0 {
            format!("/usr/bin/python3 worker_{index:04}.py --queue background")
        } else {
            format!("/usr/bin/node service_{index:04}.mjs --workspace project-{index:04}")
        };
        output.push_str(&format!("{pid} 1 02:{:02} {command}\n", index % 60));
    }
    output
}

pub fn git_status_porcelain_fixture() -> String {
    "## main\nM  src/lib.rs\n?? tests/output_metrics.rs\n".to_string()
}

pub fn git_diff_stat_fixture() -> String {
    " src/lib.rs | 300 +++++++++++++++++++++++++++++++++++++++++++++-------------\n 1 file changed, 240 insertions(+), 60 deletions(-)\n".to_string()
}

pub fn git_diff_fixture(changed_lines: usize) -> String {
    let mut output = String::new();
    output.push_str("diff --git a/src/lib.rs b/src/lib.rs\n");
    output.push_str("@@ -1,1 +1,1 @@\n");
    for index in 0..changed_lines {
        output.push_str(&format!("-old line {index:03}\n"));
        output.push_str(&format!("+new line {index:03}\n"));
    }
    output
}

pub fn git_log_fixture(commits: usize) -> String {
    let mut output = String::new();
    for index in 0..commits {
        output.push_str(&format!(
            "abc{index:03} r{index} Improve benchmark path (2 hours ago) <dev>\n"
        ));
        output.push_str("body line one\nbody line two\nbody line three\nbody line four\n");
        output.push_str("---END---\n");
    }
    output
}

pub fn cargo_compile_failure_fixture(noise_lines: usize) -> String {
    let mut output = String::new();
    for index in 0..noise_lines {
        output.push_str(&format!("   Compiling crate_{index:03} v0.1.0\n"));
    }
    output.push_str("error[E0425]: cannot find value `missing_value` in this scope\n");
    output.push_str(" --> src/lib.rs:42:9\n");
    output.push_str("  |\n");
    output.push_str("42 |         missing_value\n");
    output.push_str("  |         ^^^^^^^^^^^^^ not found in this scope\n\n");
    output.push_str("error: could not compile `demo` (lib test) due to 1 previous error\n");
    output
}

pub fn pytest_failure_fixture(noise_lines: usize) -> String {
    let mut output = String::new();
    output.push_str("=== test session starts ===\ncollected 120 items\n\n");
    for index in 0..noise_lines {
        output.push_str(&format!("tests/test_noise_{index:03}.py . [  1%]\n"));
    }
    output.push_str("=== FAILURES ===\n");
    output.push_str("___ test_alpha ___\n");
    output.push_str("> assert actual == expected\n");
    output.push_str("E AssertionError: expected stable output\n");
    output.push_str("=== short test summary info ===\n");
    output.push_str("FAILED tests/test_alpha.py::test_alpha - AssertionError\n");
    output.push_str("=== 119 passed, 1 failed in 0.42s ===\n");
    output
}

pub fn go_json_failure_fixture(pass_events: usize) -> String {
    let mut output = String::new();
    for index in 0..pass_events {
        output.push_str(&format!(
            "{{\"Action\":\"pass\",\"Package\":\"example.com/pkg{:03}\",\"Test\":\"TestPass{:03}\"}}\n",
            index % 12,
            index
        ));
    }
    output.push_str("{\"Action\":\"output\",\"Package\":\"example.com/alpha\",\"Test\":\"TestWidget\",\"Output\":\"    Error: expected 5, got 3\\n\"}\n");
    output.push_str(
        "{\"Action\":\"fail\",\"Package\":\"example.com/alpha\",\"Test\":\"TestWidget\"}\n",
    );
    output.push_str("{\"Action\":\"fail\",\"Package\":\"example.com/alpha\"}\n");
    output
}

pub fn ctest_failure_fixture(noise_lines: usize) -> String {
    let mut output = String::new();
    output.push_str("Test project build-web\n");
    for index in 0..noise_lines {
        output.push_str(&format!(
            "{{\"frame\":{index},\"status\":\"rendered\",\"payload\":\"{}\"}}\n",
            "x".repeat(80)
        ));
    }
    output.push_str("1/2 Test #1: sample-ui-dockview-perf-mouse-e2e ....***Failed 0.42 sec\n");
    output.push_str("Error: timed out waiting for dockview panel\n");
    output.push_str("50% tests passed, 1 tests failed out of 2\n");
    output.push_str("The following tests FAILED:\n");
    output.push_str("\t1 - sample-ui-dockview-perf-mouse-e2e (Failed)\n");
    output.push_str("Errors while running CTest\n");
    output
}

pub fn ctest_list_fixture(tests: usize) -> String {
    let mut output = String::new();
    output.push_str("Test project build-web\n");
    for index in 0..tests {
        output.push_str(&format!(
            "  Test #{index}: sample-ui-catalog-model-test-{index:03}\n"
        ));
    }
    output.push_str(&format!("Total Tests: {tests}\n"));
    output
}

pub fn cmake_failure_fixture(noise_lines: usize) -> String {
    let mut output = String::new();
    for index in 0..noise_lines {
        output.push_str(&format!(
            "[{:3}%] Building CXX object src/CMakeFiles/app.dir/noise_{index:03}.cpp.o\n",
            index % 100
        ));
    }
    output.push_str("FAILED: src/CMakeFiles/app.dir/runtime.cpp.o\n");
    output.push_str("clang++: error: no such file or directory: 'missing_runtime.cpp'\n");
    output.push_str("CMake Error at src/CMakeLists.txt:42 (add_executable):\n");
    output.push_str("  Cannot find source file: missing_runtime.cpp\n");
    output.push_str("ninja: build stopped: subcommand failed.\n");
    output
}

pub fn cmake_success_fixture(progress_lines: usize) -> String {
    let mut output = String::new();
    for index in 0..progress_lines {
        output.push_str(&format!(
            "[{:3}%] Building CXX object src/CMakeFiles/cx.dir/module_{index:03}.cpp.o\n",
            index % 100
        ));
    }
    output.push_str("[100%] Linking CXX executable cx\n");
    output.push_str("[100%] Built target cx\n");
    output
}

pub fn tsc_failure_fixture(noise_lines: usize) -> String {
    let mut output = String::new();
    for index in 0..noise_lines {
        output.push_str(&format!("src/generated/noise_{index:03}.ts: unchanged\n"));
    }
    output.push_str(
        "src/app.ts(10,5): error TS2322: Type 'string' is not assignable to type 'number'.\n",
    );
    output.push_str("  const count: number = routeName;\n");
    output.push_str("src/view.tsx(44,13): error TS2339: Property 'missing' does not exist on type 'ViewModel'.\n");
    output.push_str("  model.missing();\n");
    output.push_str(
        "src/view.tsx(52,9): error TS7006: Parameter 'event' implicitly has an 'any' type.\n",
    );
    output
}

pub fn log_output_fixture(entries: usize) -> String {
    let mut output = String::new();
    for index in 0..entries {
        output.push_str(&format!(
            "2026-06-18T12:00:{:02} INFO request {} completed in {}ms\n",
            index % 60,
            10_000 + index,
            20 + index
        ));
        if index % 5 == 0 {
            output.push_str(&format!(
                "2026-06-18T12:00:{:02} WARN retrying connection to /srv/app/{index}/socket\n",
                index % 60
            ));
        }
        if index % 11 == 0 {
            output.push_str(&format!(
                "2026-06-18T12:00:{:02} ERROR failed request id={} path=/srv/app/{index}/route\n",
                index % 60,
                40_000 + index
            ));
        }
    }
    output
}

#[derive(Debug, Clone)]
pub struct FindFixture {
    pub root: PathBuf,
    pub raw_rs_listing: String,
    pub path_dir_name: String,
    pub executable_name: String,
}

pub fn populate_find_fixture(root: &Path, rust_files: usize) -> FindFixture {
    let mut raw_rs_listing = String::new();
    for index in 0..rust_files {
        let module_dir = root.join("src").join(format!("module_{:02}", index % 16));
        let file = module_dir.join(format!("file_{index:04}.rs"));
        write_file(
            &file,
            &format!("pub fn fixture_{index:04}() -> usize {{ {index} }}\n"),
        );
        raw_rs_listing.push_str(&file.display().to_string());
        raw_rs_listing.push('\n');

        if index % 6 == 0 {
            write_file(
                &module_dir.join(format!("component_{index:04}.mjs")),
                "export const value = 1;\n",
            );
        }
    }

    let path_dir = root.join("app").join("node_modules").join("playwright");
    fs::create_dir_all(&path_dir).expect("create find path fixture");
    write_file(
        &path_dir.join("package.json"),
        "{\"name\":\"playwright\"}\n",
    );
    write_file(
        &root.join(".cache").join("hidden.rs"),
        "pub fn hidden() {}\n",
    );
    write_executable(
        &root.join("bin"),
        "cx-helper",
        "#!/bin/sh\nprintf 'helper\\n'\n",
    );

    FindFixture {
        root: root.to_path_buf(),
        raw_rs_listing,
        path_dir_name: path_dir.display().to_string(),
        executable_name: "cx-helper".to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct NodeCheckFixture {
    pub js_file: PathBuf,
    pub jsx_file: PathBuf,
    pub mjs_file: PathBuf,
}

pub fn write_node_check_fixtures(root: &Path) -> NodeCheckFixture {
    let src = root.join("shared").join("webgpu").join("src");
    let js_file = src.join("runtime_core").join("plain.js");
    let jsx_file = src.join("ui").join("Panel.jsx");
    let mjs_file = src.join("runtime_core").join("native_core.mjs");
    write_file(&js_file, "export const ok = 1;\n");
    write_file(
        &jsx_file,
        "export default function Panel() { return <section data-ok />; }\n",
    );
    write_file(
        &mjs_file,
        "import manifest from \"../packages/content-data/manifest.json\" assert { type: \"json\" };\nexport default manifest;\n",
    );
    NodeCheckFixture {
        js_file,
        jsx_file,
        mjs_file,
    }
}

pub fn node_import_assertion_failure_script() -> &'static str {
    r#"#!/bin/sh
last=""
for arg in "$@"; do
  last="$arg"
done
case "$last" in
  *.mjs)
    printf '%s:1 import contentDataPackageManifest from "../packages/content-data/manifest.json" assert { type: "json" };\n' "$last" >&2
    printf '^^^^^^\n' >&2
    printf "SyntaxError: Unexpected identifier 'assert'\n" >&2
    exit 1
    ;;
  *)
    exit 0
    ;;
esac
"#
}

fn restore_env(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => env::set_var(name, value),
        None => env::remove_var(name),
    }
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
