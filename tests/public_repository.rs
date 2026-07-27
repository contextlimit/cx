use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cx::cli::try_parse_from_cx_args;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn cargo_metadata_is_public_release_ready() {
    let cargo: toml::Value = toml::from_str(&read(repo_root().join("Cargo.toml"))).unwrap();
    let package = cargo
        .get("package")
        .and_then(toml::Value::as_table)
        .unwrap();

    assert_eq!(
        package.get("repository").and_then(toml::Value::as_str),
        Some("https://github.com/contextlimit/cx")
    );
    assert_eq!(
        package.get("homepage").and_then(toml::Value::as_str),
        Some("https://github.com/contextlimit/cx")
    );
    assert_eq!(
        package.get("readme").and_then(toml::Value::as_str),
        Some("README.md")
    );
    assert_eq!(
        package.get("license").and_then(toml::Value::as_str),
        Some("MIT")
    );
    assert!(package
        .get("description")
        .and_then(toml::Value::as_str)
        .is_some_and(|value| value.contains("AI coding agents")));
}

#[test]
fn public_readme_exposes_install_codex_community_and_privacy_contracts() {
    let root = repo_root();
    let readme = read(root.join("README.md"));

    for asset in [
        "docs/assets/cx-insights-overview.png",
        "docs/assets/cx-local-sqlite.png",
        "docs/assets/cx-sqlite-tables.png",
    ] {
        assert!(readme.contains(asset), "README does not reference {asset}");
        assert!(root.join(asset).is_file(), "missing public asset {asset}");
    }

    for required in [
        "independent local Rust CLI built for OpenAI Codex workflows",
        "CX is not an OpenAI product",
        "npm install -g @contextlimit/cx",
        "brew install contextlimit/tap/cx",
        "~/.codex/AGENTS.md",
        "### Agent Instruction",
        "#### Simple Codex instruction",
        "#### Advanced Codex instruction",
        "CX Insights UI preview, coming soon",
        "~/.cx/db.sqlite",
        "Local insights are **enabled by default**",
        "CX has no vendor analytics service",
        "https://discord.gg/5esGQ5qyrw",
        "https://x.com/contextlimit",
        "http://youtube.com/@contextlimit",
        "https://stackoverflow.com/users/119301/contextlimit?tab=profile",
        "https://github.com/yvgude/lean-ctx",
        "https://github.com/rtk-ai/rtk",
    ] {
        assert!(readme.contains(required), "README missing `{required}`");
    }

    let install = readme.find("## Install").unwrap();
    let agent_instruction = readme.find("### Agent Instruction").unwrap();
    let quick_start = readme.find("## Quick Start").unwrap();
    assert!(
        install < agent_instruction && agent_instruction < quick_start,
        "Agent Instruction must remain an Install subheading"
    );

    let local_files = readme.find("docs/assets/cx-local-sqlite.png").unwrap();
    let sqlite_tables = readme.find("docs/assets/cx-sqlite-tables.png").unwrap();
    let between = &readme[local_files..sqlite_tables];
    assert!(
        between.contains("</tr>") && between.contains("<tr>"),
        "local-insights screenshots must render in separate table rows"
    );
}

#[test]
fn public_readme_local_links_and_png_assets_resolve() {
    let root = repo_root();
    let readme = read(root.join("README.md"));
    let markdown_links = regex::Regex::new(r"\]\(([^)]+)\)").unwrap();
    let html_sources = regex::Regex::new(r#"\bsrc="([^"]+)""#).unwrap();

    for target in markdown_links
        .captures_iter(&readme)
        .chain(html_sources.captures_iter(&readme))
        .filter_map(|captures| captures.get(1).map(|value| value.as_str()))
    {
        if target.starts_with("https://")
            || target.starts_with("http://")
            || target.starts_with('#')
            || target.starts_with("mailto:")
        {
            continue;
        }
        let path = target.split_once('#').map_or(target, |(path, _)| path);
        assert!(
            root.join(path).exists(),
            "README local link is missing: {target}"
        );
    }

    for (path, expected_width, expected_height) in [
        ("docs/assets/cx-insights-overview.png", 875, 710),
        ("docs/assets/cx-local-sqlite.png", 659, 238),
        ("docs/assets/cx-sqlite-tables.png", 865, 704),
    ] {
        let bytes = fs::read(root.join(path)).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{path} is not a PNG");
        assert_eq!(
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            expected_width
        );
        assert_eq!(
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
            expected_height
        );
    }
}

#[test]
fn tracked_markdown_local_links_resolve() {
    let root = repo_root();
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let markdown_links = regex::Regex::new(r"\]\(([^)]+)\)").unwrap();
    let html_links = regex::Regex::new(r#"\b(?:href|src)="([^"]+)""#).unwrap();
    let mut missing = Vec::new();

    for relative in String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|path| path.ends_with(".md"))
    {
        let document = root.join(relative);
        let content = read(&document);
        for target in markdown_links
            .captures_iter(&content)
            .chain(html_links.captures_iter(&content))
            .filter_map(|captures| captures.get(1).map(|value| value.as_str()))
        {
            if target.starts_with("https://")
                || target.starts_with("http://")
                || target.starts_with('#')
                || target.starts_with("mailto:")
            {
                continue;
            }
            let local = target
                .trim_matches(['<', '>'])
                .split_once('#')
                .map_or(target, |(path, _)| path);
            if local.is_empty() {
                continue;
            }
            let resolved = document.parent().unwrap().join(local);
            if !resolved.exists() {
                missing.push(format!("{relative}: {target}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "tracked Markdown contains missing local links: {missing:?}"
    );
}

#[test]
fn public_readme_command_examples_parse() {
    let commands = [
        &["cx", "--", "git", "status"][..],
        &["cx", "--", "git", "diff"][..],
        &["cx", "--", "rg", "-n", "Command::output", "src"][..],
        &["cx", "--", "cargo", "test"][..],
        &["cx", "--", "ps", "-axo", "pid,ppid,etime,command"][..],
        &[
            "cx",
            "insights",
            "settings",
            "--set",
            "record_invocations=true",
            "--set",
            "record_command_shape=true",
        ][..],
        &["cx", "insights", "presentation"][..],
        &[
            "cx", "insights", "export", "--format", "json", "--limit", "25",
        ][..],
    ];

    for argv in commands {
        try_parse_from_cx_args(argv.iter().copied())
            .unwrap_or_else(|error| panic!("README command does not parse: {argv:?}: {error}"));
    }
}

#[test]
fn public_examples_use_current_command_surfaces_and_source_paths() {
    let root = repo_root();
    let examples = root.join("examples");
    for entry in fs::read_dir(&examples).unwrap() {
        let path = entry.unwrap().path();
        if path.extension() != Some(OsStr::new("md")) {
            continue;
        }
        for line in read(&path).lines().filter(|line| line.starts_with("cx ")) {
            assert!(
                line.starts_with("cx -- ")
                    || line.starts_with("cx insights ")
                    || line.starts_with("cx report ")
                    || line.starts_with("cx sh "),
                "{} contains a stale or unsupported CX command surface `{line}`",
                path.display()
            );
        }
    }

    let read_examples = read(examples.join("read-and-grep.md"));
    for source in [
        "src/lib.rs",
        "src/dispatch.rs",
        "src/commands/read/mod.rs",
        "src/support/runner.rs",
    ] {
        assert!(
            root.join(source).is_file(),
            "missing example source {source}"
        );
        assert!(
            read_examples.contains(source),
            "read-and-grep example must reference {source}"
        );
    }
}

#[test]
fn release_npm_and_homebrew_surfaces_share_one_asset_contract() {
    let root = repo_root();
    let cargo: toml::Value = toml::from_str(&read(root.join("Cargo.toml"))).unwrap();
    let cargo_version = cargo["package"]["version"].as_str().unwrap();
    let template = read(root.join("packaging/homebrew/Formula/cx.rb.in"));
    let guide = read(root.join("packaging/homebrew/README.md"));
    let release = read(root.join(".github/workflows/release.yml"));
    let npm: serde_json::Value =
        serde_json::from_str(&read(root.join("packaging/npm/package.json"))).unwrap();

    for token in [
        "@VERSION@",
        "@DARWIN_ARM64_URL@",
        "@DARWIN_ARM64_SHA256@",
        "@DARWIN_X64_URL@",
        "@DARWIN_X64_SHA256@",
        "@LINUX_ARM64_URL@",
        "@LINUX_ARM64_SHA256@",
        "@LINUX_X64_URL@",
        "@LINUX_X64_SHA256@",
    ] {
        assert!(template.contains(token), "formula template missing {token}");
    }
    assert!(template.contains("homepage \"https://github.com/contextlimit/cx\""));
    assert!(template.contains("using: :nounzip"));
    assert!(!template.contains("depends_on \"rust\""));
    assert!(guide.contains("contextlimit/homebrew-tap"));
    assert!(guide.contains("brew install contextlimit/tap/cx"));

    for target in ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"] {
        assert!(
            release.contains(target),
            "release workflow missing {target}"
        );
    }
    for runner in [
        "macos-15",
        "macos-15-intel",
        "ubuntu-24.04-arm",
        "ubuntu-24.04",
    ] {
        assert!(
            release.contains(runner),
            "release workflow missing {runner}"
        );
    }
    assert!(release.contains("actions/upload-artifact@v7"));
    assert!(release.contains("actions/download-artifact@v8"));
    assert!(release.contains("cx-${version}-source.tar.gz"));
    assert!(release.contains("checksums.txt"));
    assert!(release.contains("gh release create"));
    assert!(release.contains("--draft=false"));

    assert_eq!(npm["name"], "@contextlimit/cx");
    assert_eq!(npm["version"], cargo_version);
    assert_eq!(npm["publishConfig"]["access"], "public");
    assert_eq!(npm["bin"]["cx"], "bin/cx.js");
    assert!(npm.get("dependencies").is_none());
    assert_eq!(
        read(root.join("packaging/npm/LICENSE")),
        read(root.join("LICENSE"))
    );
}

#[test]
fn homebrew_renderer_requires_complete_checksums_and_explicit_local_proof() {
    let root = repo_root();
    let script = root.join("packaging/homebrew/render-formula.sh");
    let temp = tempfile::tempdir().unwrap();
    let output_path = temp.path().join("cx.rb");
    let checksums = temp.path().join("checksums.txt");
    let manifest = [
        ("darwin-arm64", "a"),
        ("darwin-x64", "b"),
        ("linux-arm64", "c"),
        ("linux-x64", "d"),
    ]
    .into_iter()
    .map(|(target, digit)| format!("{}  cx-v0.1.0-{target}\n", digit.repeat(64)))
    .collect::<String>();
    fs::write(&checksums, manifest).unwrap();
    let local_base = format!("file://{}", temp.path().display());

    let denied = Command::new(&script)
        .args(["0.1.0"])
        .arg(&checksums)
        .arg(&output_path)
        .arg(&local_base)
        .output()
        .unwrap();
    assert_eq!(denied.status.code(), Some(2));
    assert!(!output_path.exists());

    let allowed = Command::new(&script)
        .args(["0.1.0"])
        .arg(&checksums)
        .arg(&output_path)
        .arg(&local_base)
        .env("CX_HOMEBREW_ALLOW_FILE_URL", "1")
        .output()
        .unwrap();
    assert!(allowed.status.success(), "{allowed:?}");
    let formula = read(&output_path);
    for target in ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"] {
        assert!(formula.contains(&format!("cx-v0.1.0-{target}")));
    }
    assert!(!formula.contains('@'));
}

#[test]
fn tracked_tree_excludes_private_runtime_state() {
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(repo_root())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let forbidden = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|path| {
            path == &".DS_Store"
                || path.starts_with(".tmp/")
                || path.ends_with(".sqlite")
                || path.ends_with(".sqlite-wal")
                || path.ends_with(".sqlite-shm")
        })
        .map(str::to_string)
        .collect::<Vec<_>>();

    assert!(
        forbidden.is_empty(),
        "tracked private runtime state: {forbidden:?}"
    );
}

#[test]
fn tracked_text_is_free_of_private_branding_hosts_and_wrong_identity() {
    let root = repo_root();
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let forbidden = [
        ["ar", "ken"].concat(),
        ["tele", "gram"].concat(),
        ["t", ".me/"].concat(),
        ["context", "i", "imit"].concat(),
        ["contextlimit", "-admin"].concat(),
        ["/users/", "dev"].concat(),
        ["/volumes/", "ude"].concat(),
        ["studio", "-1"].concat(),
        ["cerebro", "-1"].concat(),
        ["pro", "-1"].concat(),
    ];
    let mut offenders = Vec::new();

    for relative in String::from_utf8(output.stdout).unwrap().lines() {
        let path = root.join(relative);
        if !is_public_text_file(&path) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let lower = content.to_ascii_lowercase();
        for term in &forbidden {
            if lower.contains(term) {
                offenders.push((relative.to_string(), term.clone()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "tracked public text contains private branding or machine identity: {offenders:?}"
    );
}

fn is_public_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some(
            "rs" | "toml"
                | "lock"
                | "json"
                | "md"
                | "txt"
                | "sh"
                | "js"
                | "cjs"
                | "mjs"
                | "jsx"
                | "ts"
                | "tsx"
                | "yaml"
                | "yml"
        )
    ) || matches!(
        path.file_name().and_then(OsStr::to_str),
        Some("LICENSE" | ".gitignore")
    )
}
