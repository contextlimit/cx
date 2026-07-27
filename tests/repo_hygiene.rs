use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_HAND_WRITTEN_RUST_LINES: usize = 2_000;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn source_tree_uses_cx_naming_only() {
    let root = repo_root();
    let legacy_names = [
        ["a", "xl"].concat(),
        ["A", "xl"].concat(),
        ["AX", "L"].concat(),
    ];
    let mut files = Vec::new();
    collect_files(&root.join("src"), &mut files);
    collect_files(&root.join("benches"), &mut files);
    collect_files(&root.join("examples"), &mut files);
    files.push(root.join("install.sh"));
    files.push(root.join("scripts/install.sh"));

    for file in files {
        if !is_text_source_file(&file) {
            continue;
        }
        let content = read(&file);
        for legacy_name in &legacy_names {
            assert!(
                !content.contains(legacy_name),
                "{} contains legacy binary name variant `{legacy_name}`",
                file.display()
            );
        }
    }
}

#[test]
fn insights_documentation_matches_schema_constants() {
    let root = repo_root();
    let database_schema = rust_u64_constant(
        &read(root.join("src/support/insights_store.rs")),
        "INSIGHTS_DATABASE_SCHEMA_VERSION",
    );
    let export_schema = rust_u64_constant(
        &read(root.join("src/commands/insights/export.rs")),
        "EXPORT_SCHEMA_VERSION",
    );
    let dashboard_schema = rust_u64_constant(
        &read(root.join("src/commands/insights_dashboard.rs")),
        "DASHBOARD_SCHEMA_VERSION",
    );
    let documentation = read(root.join("docs/features/insights.md"));
    let expected = format!(
        "The current SQLite schema version is `{database_schema}`, the export schema version is \
         `{export_schema}`, and the dashboard schema version is `{dashboard_schema}`."
    );
    let normalized_documentation = documentation
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        normalized_documentation.contains(&expected),
        "docs/features/insights.md must contain `{expected}`"
    );
}

#[test]
fn tracked_tree_does_not_include_desktop_store_files() {
    let output = Command::new("git")
        .args(["ls-files"])
        .current_dir(repo_root())
        .output()
        .expect("git ls-files");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let tracked_desktop_files = stdout
        .lines()
        .filter(|line| Path::new(line).file_name() == Some(OsStr::new(".DS_Store")))
        .collect::<Vec<_>>();
    assert!(
        tracked_desktop_files.is_empty(),
        "tracked .DS_Store files: {:?}",
        tracked_desktop_files
    );
}

#[test]
fn hand_written_rust_files_stay_under_size_guideline() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_files(&root.join("src"), &mut files);
    collect_files(&root.join("benches"), &mut files);
    collect_files(&root.join("tests"), &mut files);
    collect_files(&root.join("examples"), &mut files);

    let oversized = files
        .into_iter()
        .filter(|file| file.extension() == Some(OsStr::new("rs")))
        .filter(|file| !is_generated_path(file))
        .filter_map(|file| {
            let line_count = read(&file).lines().count();
            (line_count > MAX_HAND_WRITTEN_RUST_LINES).then_some((file, line_count))
        })
        .collect::<Vec<_>>();

    assert!(
        oversized.is_empty(),
        "hand-written Rust files exceed {MAX_HAND_WRITTEN_RUST_LINES} lines: {:?}",
        oversized
    );
}

#[test]
fn production_rust_uses_runner_capture_instead_of_command_output() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_files(&root.join("src"), &mut files);

    let offenders = files
        .into_iter()
        .filter(|file| file.extension() == Some(OsStr::new("rs")))
        .filter(|file| !is_generated_path(file))
        .filter_map(|file| {
            let content = read(&file);
            (content.contains(".output()") || content.contains("Command::output")).then_some(file)
        })
        .collect::<Vec<_>>();

    assert!(
        offenders.is_empty(),
        "production Rust should use support::runner capture helpers instead of pipe-backed Command::output: {:?}",
        offenders
    );
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn is_text_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some(
            "rs" | "toml"
                | "json"
                | "md"
                | "txt"
                | "sh"
                | "js"
                | "mjs"
                | "jsx"
                | "ts"
                | "tsx"
                | "yaml"
                | "yml"
        )
    ) || path.file_name() == Some(OsStr::new("install.sh"))
}

fn is_generated_path(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str();
        value == OsStr::new("generated") || value == OsStr::new("codegen")
    })
}

fn rust_u64_constant(source: &str, name: &str) -> u64 {
    source
        .lines()
        .find(|line| line.contains(name))
        .and_then(|line| line.split_once('='))
        .map(|(_, value)| value.trim().trim_end_matches(';'))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("failed to parse Rust u64 constant `{name}`"))
}
