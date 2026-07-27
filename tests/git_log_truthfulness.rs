use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::{tempdir, TempDir};

struct MergeRepo {
    _temp: TempDir,
    root: PathBuf,
}

impl MergeRepo {
    fn new() -> Self {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        git_ok(&root, &["init", "-q"]);
        git_ok(&root, &["config", "user.name", "CX Test"]);
        git_ok(&root, &["config", "user.email", "cx-test@example.invalid"]);
        git_ok(&root, &["branch", "-M", "main"]);

        fs::write(root.join("base.txt"), "base\n").unwrap();
        git_ok_at(&root, &["add", "base.txt"], 1);
        git_ok_at(&root, &["commit", "-q", "-m", "initial"], 1);

        git_ok(&root, &["checkout", "-q", "-b", "topic"]);
        fs::write(root.join("topic.txt"), "topic\n").unwrap();
        git_ok_at(&root, &["add", "topic.txt"], 2);
        git_ok_at(&root, &["commit", "-q", "-m", "topic"], 2);

        git_ok(&root, &["checkout", "-q", "main"]);
        fs::write(root.join("main.txt"), "main\n").unwrap();
        git_ok_at(&root, &["add", "main.txt"], 3);
        git_ok_at(&root, &["commit", "-q", "-m", "main"], 3);
        git_ok_at(
            &root,
            &["merge", "-q", "--no-ff", "topic", "-m", "merge topic"],
            4,
        );

        let parents = git_output(&root, &["rev-list", "--parents", "-n", "1", "HEAD"]);
        assert!(parents.status.success(), "{parents:?}");
        assert_eq!(
            String::from_utf8_lossy(&parents.stdout)
                .split_whitespace()
                .count(),
            3,
            "fixture HEAD must have exactly two parents"
        );

        Self { _temp: temp, root }
    }

    fn native_log(&self, args: &[&str]) -> Output {
        git_output(&self.root, &[&["log"], args].concat())
    }

    fn cx_log(&self, cx: &Path, home: &Path, args: &[&str]) -> Output {
        Command::new(cx)
            .args(["--", "git", "log"])
            .args(args)
            .current_dir(&self.root)
            .env("HOME", home)
            .env("CX_DISABLE_INSIGHTS", "1")
            .output()
            .unwrap()
    }
}

#[test]
fn auto_git_log_preserves_merge_head_and_first_parent_selection() {
    let fixture = MergeRepo::new();
    let home = tempdir().unwrap();
    let cx = Path::new(env!("CARGO_BIN_EXE_cx"));

    assert_log_parity(&fixture, cx, home.path(), &["-n", "1", "--oneline"]);
    assert_log_parity(&fixture, cx, home.path(), &["-n", "1", "--format=%H"]);
    assert_log_parity(
        &fixture,
        cx,
        home.path(),
        &["--first-parent", "--oneline", "--decorate", "-n", "3"],
    );

    let head = git_output(&fixture.root, &["rev-parse", "HEAD"]);
    let native = fixture.native_log(&["-n", "1", "--format=%H"]);
    assert_eq!(native.stdout, head.stdout);
}

#[test]
fn installed_wrapper_preserves_merge_head_and_first_parent_selection() {
    let fixture = MergeRepo::new();
    let home = tempdir().unwrap();
    let install = Command::new("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh"))
        .env("HOME", home.path())
        .env("CX_INSTALL_SKIP_BUILD", "1")
        .env("CX_INSTALL_BIN_PATH", env!("CARGO_BIN_EXE_cx"))
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let installed = home.path().join(".local/bin/cx");
    assert_log_parity(&fixture, &installed, home.path(), &["-n", "1", "--oneline"]);
    assert_log_parity(
        &fixture,
        &installed,
        home.path(),
        &["--first-parent", "--oneline", "--decorate", "-n", "3"],
    );
    assert!(!home.path().join(".cx/db.sqlite").exists());
}

fn assert_log_parity(fixture: &MergeRepo, cx: &Path, home: &Path, args: &[&str]) {
    let native = fixture.native_log(args);
    let wrapped = fixture.cx_log(cx, home, args);
    assert!(native.status.success(), "{native:?}");
    assert!(wrapped.status.success(), "{wrapped:?}");
    assert_eq!(wrapped.stdout, native.stdout, "args: {args:?}");
    assert_eq!(wrapped.stderr, native.stderr, "args: {args:?}");
}

fn git_ok(root: &Path, args: &[&str]) {
    let output = git_output(root, args);
    assert!(output.status.success(), "{output:?}");
}

fn git_ok_at(root: &Path, args: &[&str], second: u8) {
    let timestamp = format!("2001-01-01T00:00:0{second}Z");
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_DATE", &timestamp)
        .env("GIT_COMMITTER_DATE", &timestamp)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn git_output(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}
