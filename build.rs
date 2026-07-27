use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let revision = resolve_git_revision(&manifest).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=CX_BUILD_REVISION={revision}");
}

fn resolve_git_revision(manifest: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(manifest)?;
    let head_path = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());
    let head = fs::read_to_string(&head_path).ok()?;
    let head = head.trim();
    let revision = if let Some(reference) = head.strip_prefix("ref: ") {
        resolve_reference(&git_dir, reference)?
    } else {
        head.to_string()
    };
    let revision = revision.trim();
    revision
        .chars()
        .all(|ch| ch.is_ascii_hexdigit())
        .then(|| revision.chars().take(12).collect())
}

fn resolve_git_dir(manifest: &Path) -> Option<PathBuf> {
    let dot_git = manifest.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let pointer = fs::read_to_string(dot_git).ok()?;
    let path = pointer.trim().strip_prefix("gitdir: ")?;
    let path = PathBuf::from(path);
    Some(if path.is_absolute() {
        path
    } else {
        manifest.join(path)
    })
}

fn resolve_reference(git_dir: &Path, reference: &str) -> Option<String> {
    for root in git_roots(git_dir) {
        let path = root.join(reference);
        println!("cargo:rerun-if-changed={}", path.display());
        if let Ok(revision) = fs::read_to_string(&path) {
            return Some(revision.trim().to_string());
        }
        if let Some(revision) = packed_reference(&root.join("packed-refs"), reference) {
            return Some(revision);
        }
    }
    None
}

fn git_roots(git_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![git_dir.to_path_buf()];
    if let Ok(common) = fs::read_to_string(git_dir.join("commondir")) {
        let common = PathBuf::from(common.trim());
        let common = if common.is_absolute() {
            common
        } else {
            git_dir.join(common)
        };
        if common != git_dir {
            roots.push(common);
        }
    }
    roots
}

fn packed_reference(path: &Path, reference: &str) -> Option<String> {
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(path)
        .ok()?
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .find_map(|line| {
            let (revision, candidate) = line.split_once(' ')?;
            (candidate == reference).then(|| revision.to_string())
        })
}
