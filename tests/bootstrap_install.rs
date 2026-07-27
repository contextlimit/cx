#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::tempdir;

const VERSION: &str = "0.1.1";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn installer() -> PathBuf {
    repo_root().join("install.sh")
}

fn make_executable(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn platform_asset(version: &str) -> String {
    let operating_system = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => panic!("unsupported test operating system: {other}"),
    };
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => panic!("unsupported test architecture: {other}"),
    };
    format!("cx-v{version}-{operating_system}-{architecture}")
}

fn sha256(path: &Path) -> String {
    let sha256sum = Command::new("sha256sum").arg(path).output();
    if let Ok(output) = sha256sum {
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .to_string();
        }
    }

    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn write_release_asset(release_dir: &Path, body: &str) -> PathBuf {
    fs::create_dir_all(release_dir).unwrap();
    let asset = release_dir.join(platform_asset(VERSION));
    fs::write(&asset, body).unwrap();
    make_executable(&asset);
    let checksum = sha256(&asset);
    fs::write(
        release_dir.join("checksums.txt"),
        format!(
            "{checksum}  {}\n",
            asset.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();
    asset
}

fn run_binary_install(home: &Path, temp_root: &Path, releases: &Path) -> Output {
    Command::new("sh")
        .arg(installer())
        .env("HOME", home)
        .env("TMPDIR", temp_root)
        .env("PATH", "/usr/bin:/bin")
        .env(
            "CX_INSTALL_RELEASE_ROOT",
            format!("file://{}", releases.display()),
        )
        .env("CX_INSTALL_ALLOW_FILE_URL", "1")
        .env_remove("CX_INSTALL_VERSION")
        .output()
        .unwrap()
}

#[test]
fn bootstrap_installs_latest_verified_binary_without_replacing_insights() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let temp_root = temp.path().join("tmp");
    let releases = temp.path().join("releases");
    let latest = releases.join("latest/download");
    let db_path = home.join(".cx/db.sqlite");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&temp_root).unwrap();
    fs::write(&db_path, b"existing insights ledger").unwrap();
    write_release_asset(
        &latest,
        "#!/bin/sh\ncase \"${1:-}\" in\n  --version) printf 'cx 0.1.1 (fixture)\\n' ;;\n  *) printf 'fixture command\\n' ;;\nesac\n",
    );

    let output = run_binary_install(&home, &temp_root, &releases);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(&db_path).unwrap(), b"existing insights ledger");
    assert!(home.join(".cx/bin/cx").is_file());
    assert!(home.join(".local/bin/cx").is_file());
    assert!(home.join(".config/cx").is_dir());
    assert!(home.join(".cx/cache").is_dir());
    assert!(fs::read_dir(&temp_root).unwrap().next().is_none());

    let installed = Command::new(home.join(".local/bin/cx"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(installed.status.success(), "{installed:?}");
    assert_eq!(
        String::from_utf8(installed.stdout).unwrap(),
        "cx 0.1.1 (fixture)\n"
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("installed cx 0.1.1 (fixture)"));
    assert!(stdout.contains(&home.join(".cx/bin/cx").display().to_string()));
}

#[test]
fn bootstrap_rejects_a_binary_with_the_wrong_checksum() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let temp_root = temp.path().join("tmp");
    let releases = temp.path().join("releases");
    let latest = releases.join("latest/download");
    fs::create_dir_all(&temp_root).unwrap();
    let asset = write_release_asset(&latest, "#!/bin/sh\nprintf 'cx 0.1.1 (fixture)\\n'\n");
    fs::write(
        latest.join("checksums.txt"),
        format!(
            "{}  {}\n",
            "0".repeat(64),
            asset.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();

    let output = run_binary_install(&home, &temp_root, &releases);

    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum mismatch"));
    assert!(!home.join(".cx/bin/cx").exists());
    assert!(!home.join(".cx/db.sqlite").exists());
    assert!(fs::read_dir(&temp_root).unwrap().next().is_none());
}

#[test]
fn bootstrap_source_mode_clones_the_tag_and_builds_locked_release() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let temp_root = temp.path().join("tmp");
    let releases = temp.path().join("releases");
    let release = releases.join(format!("download/v{VERSION}"));
    let fake_bin = temp.path().join("bin");
    let git_log = temp.path().join("git.log");
    let cargo_log = temp.path().join("cargo.log");
    fs::create_dir_all(&release).unwrap();
    fs::create_dir_all(&temp_root).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();
    fs::write(
        release.join("checksums.txt"),
        format!("{}  {}\n", "0".repeat(64), platform_asset(VERSION)),
    )
    .unwrap();

    let git = fake_bin.join("git");
    fs::write(
        &git,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$CX_INSTALL_TEST_GIT_LOG\"\nfor value do destination=$value; done\nmkdir -p \"$destination\"\n: > \"$destination/Cargo.toml\"\n",
    )
    .unwrap();
    make_executable(&git);

    let cargo = fake_bin.join("cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$CX_INSTALL_TEST_CARGO_LOG\"\nmanifest=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--manifest-path\" ]; then shift; manifest=$1; fi\n  shift\ndone\nroot=$(dirname \"$manifest\")\nmkdir -p \"$root/target/release\"\ncat > \"$root/target/release/cx\" <<'CX'\n#!/bin/sh\nprintf 'cx 0.1.1 (source-fixture)\\n'\nCX\nchmod 755 \"$root/target/release/cx\"\n",
    )
    .unwrap();
    make_executable(&cargo);

    let output = Command::new("sh")
        .arg(installer())
        .env("HOME", &home)
        .env("TMPDIR", &temp_root)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("CX_INSTALL_METHOD", "source")
        .env("CX_INSTALL_VERSION", VERSION)
        .env(
            "CX_INSTALL_RELEASE_ROOT",
            format!("file://{}", releases.display()),
        )
        .env("CX_INSTALL_ALLOW_FILE_URL", "1")
        .env(
            "CX_INSTALL_REPOSITORY_URL",
            "https://github.com/contextlimit/cx.git",
        )
        .env("CX_INSTALL_TEST_GIT_LOG", &git_log)
        .env("CX_INSTALL_TEST_CARGO_LOG", &cargo_log)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(fs::read_to_string(git_log)
        .unwrap()
        .contains("clone --depth 1 --branch v0.1.1 --single-branch"));
    assert!(fs::read_to_string(cargo_log)
        .unwrap()
        .contains("build --release --locked --bin cx --manifest-path"));
    assert!(!home.join(".cx/db.sqlite").exists());
    assert!(fs::read_dir(&temp_root).unwrap().next().is_none());

    let installed = Command::new(home.join(".local/bin/cx"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(installed.status.success(), "{installed:?}");
    assert_eq!(
        String::from_utf8(installed.stdout).unwrap(),
        "cx 0.1.1 (source-fixture)\n"
    );
}
