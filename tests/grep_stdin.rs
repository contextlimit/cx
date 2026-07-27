#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

#[test]
fn rg_alias_reads_redirected_stdin_when_no_path_is_given() {
    let fixture = GrepStdinFixture::new("rg");
    let mut child = fixture
        .command()
        .args(["--", "rg", "^CX_PIPELINE_PROBE="])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"CX_PIPELINE_PROBE=1\nNOPE=2\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"1:CX_PIPELINE_PROBE=1\n");
    assert!(fixture.tool_args().lines().any(|line| line == "-"));
}

#[test]
fn rg_alias_defaults_to_current_directory_for_dev_null_stdin() {
    let fixture = GrepStdinFixture::new("rg");
    let output = fixture
        .command()
        .args(["--", "rg", "needle"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"fixture.rs:2:needle\n");
    assert!(fixture.tool_args().lines().any(|line| line == "."));
}

#[test]
fn rg_alias_defaults_to_current_directory_for_an_empty_pipe() {
    let fixture = GrepStdinFixture::new("rg");
    let mut child = fixture
        .command()
        .args(["--", "rg", "needle"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"fixture.rs:2:needle\n");
    assert!(fixture.tool_args().lines().any(|line| line == "."));
}

#[test]
fn grep_fallback_reads_redirected_stdin_when_rg_is_unavailable() {
    let fixture = GrepStdinFixture::new("grep");
    let mut child = fixture
        .command()
        .args(["--", "grep", "^CX_PIPELINE_PROBE="])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"CX_PIPELINE_PROBE=1\nNOPE=2\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"1:CX_PIPELINE_PROBE=1\n");
    assert!(fixture.tool_args().lines().any(|line| line == "-"));
}

#[test]
fn regex_retry_replays_captured_stdin_to_grep() {
    let fixture = GrepStdinFixture::new("grep");
    fixture.write_tool(
        "rg",
        "#!/bin/sh\nprintf 'regex parse error: fixture rejection\\n' >&2\nexit 2\n",
    );
    let mut child = fixture
        .command()
        .args(["--", "rg", "("])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"CX_PIPELINE_PROBE=1\nNOPE=2\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"1:CX_PIPELINE_PROBE=1\n");
    assert!(fixture.tool_args().lines().any(|line| line == "-"));
}

struct GrepStdinFixture {
    _temp: tempfile::TempDir,
    bin: std::path::PathBuf,
    home: std::path::PathBuf,
    args_file: std::path::PathBuf,
}

impl GrepStdinFixture {
    fn new(tool_name: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        let home = temp.path().join("home");
        let args_file = temp.path().join("search-args.txt");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        let fixture = Self {
            _temp: temp,
            bin,
            home,
            args_file,
        };
        fixture.write_tool(
            tool_name,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CX_SEARCH_ARGS_FILE\"\nstdin_mode=0\nfor arg in \"$@\"; do\n  if [ \"$arg\" = - ]; then stdin_mode=1; fi\ndone\nif [ \"$stdin_mode\" = 1 ]; then\n  IFS= read -r line\n  printf '1:%s\\n' \"$line\"\nelse\n  printf 'fixture.rs:2:needle\\n'\nfi\n",
        );
        fixture
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
        command
            .env("PATH", &self.bin)
            .env("HOME", &self.home)
            .env("CX_SEARCH_ARGS_FILE", &self.args_file)
            .env("CX_DISABLE_INSIGHTS", "1")
            .env("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
        command
    }

    fn tool_args(&self) -> String {
        fs::read_to_string(&self.args_file).unwrap()
    }

    fn write_tool(&self, name: &str, body: &str) {
        let tool = self.bin.join(name);
        fs::write(&tool, body).unwrap();
        let mut permissions = fs::metadata(&tool).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(tool, permissions).unwrap();
    }
}
