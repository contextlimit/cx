#![cfg(unix)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use cx::commands::grep;
use tempfile::tempdir;

const DESCRIPTOR_SLEEP_SECS: u64 = 4;
const RETURN_DEADLINE: Duration = Duration::from_millis(1_500);

#[test]
fn rg_search_capture_returns_before_descriptor_holding_descendant_exits() {
    let fixture = GrepCaptureFixture::new("rg-search");
    let pid_file = fixture.path("rg-search.pid");
    let stdout_file = fixture.path("rg-search.out");
    write_file(&stdout_file, "src/app.rs:12:let needle = true;\n");
    write_executable(
        &fixture.bin,
        "rg",
        &descriptor_holding_output_script(&pid_file, &stdout_file, 0),
    );
    let _env = EnvGuard::set(&fixture.bin, &fixture.home);

    let (elapsed, output) = timed(|| {
        grep::run_many(
            &strings(&["needle"]),
            &strings(&["src"]),
            &grep::GrepOptions::default(),
        )
        .unwrap()
    });
    kill_pid_file(&pid_file);

    assert_returned_before_descendant("rg search", elapsed);
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout, "src/app.rs:12:let needle = true;");
}

#[test]
fn rg_files_capture_returns_before_descriptor_holding_descendant_exits() {
    let fixture = GrepCaptureFixture::new("rg-files");
    let pid_file = fixture.path("rg-files.pid");
    let stdout_file = fixture.path("rg-files.out");
    write_file(&stdout_file, "src/app.rs\nsrc/view.jsx\n");
    write_executable(
        &fixture.bin,
        "rg",
        &descriptor_holding_output_script(&pid_file, &stdout_file, 0),
    );
    let _env = EnvGuard::set(&fixture.bin, &fixture.home);

    let (elapsed, output) =
        timed(|| grep::list_files(&strings(&["src"]), &grep::GrepOptions::default()).unwrap());
    kill_pid_file(&pid_file);

    assert_returned_before_descendant("rg --files", elapsed);
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout.contains("2 files"));
    assert!(output.stdout.contains("src/view.jsx"));
}

#[test]
fn grep_fallback_capture_returns_before_descriptor_holding_descendant_exits() {
    let fixture = GrepCaptureFixture::new("fallback");
    let pid_file = fixture.path("grep-fallback.pid");
    let stdout_file = fixture.path("grep-fallback.out");
    write_file(
        &stdout_file,
        "src/fallback.rs:7:let fallback_needle = true;\n",
    );
    write_executable(
        &fixture.bin,
        "rg",
        "#!/bin/sh\nprintf 'regex parse error: unclosed group\\n' >&2\nexit 2\n",
    );
    write_executable(
        &fixture.bin,
        "grep",
        &descriptor_holding_output_script(&pid_file, &stdout_file, 0),
    );
    let _env = EnvGuard::set(&fixture.bin, &fixture.home);

    let (elapsed, output) = timed(|| {
        grep::run_many(
            &strings(&["fallback_needle"]),
            &strings(&["src"]),
            &grep::GrepOptions::default(),
        )
        .unwrap()
    });
    kill_pid_file(&pid_file);

    assert_returned_before_descendant("grep fallback", elapsed);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        output.stdout,
        "src/fallback.rs:7:let fallback_needle = true;"
    );
}

struct GrepCaptureFixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    bin: PathBuf,
    home: PathBuf,
}

impl GrepCaptureFixture {
    fn new(label: &str) -> Self {
        let temp = tempdir().unwrap();
        let root = temp.path().join(label);
        let bin = root.join("bin");
        let home = root.join("home");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        Self {
            _temp: temp,
            root,
            bin,
            home,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

fn descriptor_holding_output_script(pid_file: &Path, stdout_file: &Path, exit_code: i32) -> String {
    format!(
        "#!/bin/sh\n\
(/bin/sleep {DESCRIPTOR_SLEEP_SECS}) &\n\
printf '%s\\n' \"$!\" > {}\n\
/bin/cat {}\n\
exit {exit_code}\n",
        shell_quote(pid_file),
        shell_quote(stdout_file)
    )
}

fn timed<T>(action: impl FnOnce() -> T) -> (Duration, T) {
    let started = Instant::now();
    let output = action();
    (started.elapsed(), output)
}

fn assert_returned_before_descendant(label: &str, elapsed: Duration) {
    assert!(
        elapsed < RETURN_DEADLINE,
        "{label} waited {elapsed:?}; file-backed capture should return before the descriptor-holding descendant exits"
    );
}

fn kill_pid_file(pid_file: &Path) {
    let Ok(pid) = fs::read_to_string(pid_file) else {
        return;
    };
    let pid = pid.trim();
    if !pid.is_empty() {
        let _ = Command::new("/bin/kill").arg(pid).status();
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn write_executable(dir: &Path, name: &str, body: &str) {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn shell_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    path: Option<OsString>,
    disable_fallback: Option<OsString>,
    extra_fallback: Option<OsString>,
    home: Option<OsString>,
}

impl EnvGuard {
    fn set(bin: &Path, home: &Path) -> Self {
        let lock = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let guard = Self {
            _lock: lock,
            path: env::var_os("PATH"),
            disable_fallback: env::var_os("CX_DISABLE_TOOL_FALLBACK_PATHS"),
            extra_fallback: env::var_os("CX_TOOL_FALLBACK_PATHS"),
            home: env::var_os("HOME"),
        };
        env::set_var("PATH", bin);
        env::set_var("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
        env::remove_var("CX_TOOL_FALLBACK_PATHS");
        env::set_var("HOME", home);
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        restore_env("PATH", self.path.take());
        restore_env(
            "CX_DISABLE_TOOL_FALLBACK_PATHS",
            self.disable_fallback.take(),
        );
        restore_env("CX_TOOL_FALLBACK_PATHS", self.extra_fallback.take());
        restore_env("HOME", self.home.take());
    }
}

fn restore_env(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => env::set_var(name, value),
        None => env::remove_var(name),
    }
}
