use std::fs;
use std::path::{Path, PathBuf};

use cx::cli::try_parse_from_cx_args;

struct DocumentedCommand {
    page: &'static str,
    line: &'static str,
    argv: &'static [&'static str],
}

const DOCUMENTED_COMMANDS: &[DocumentedCommand] = &[
    DocumentedCommand {
        page: "cargo.md",
        line: "cx cargo test -p cx --lib",
        argv: &["cx", "cargo", "test", "-p", "cx", "--lib"],
    },
    DocumentedCommand {
        page: "cmake-ctest.md",
        line: "cx -- cmake --build build-web --target sample-ui -j8",
        argv: &[
            "cx",
            "--",
            "cmake",
            "--build",
            "build-web",
            "--target",
            "sample-ui",
            "-j8",
        ],
    },
    DocumentedCommand {
        page: "cmake-ctest.md",
        line: "cx ctest --test-dir build -R unit",
        argv: &["cx", "ctest", "--test-dir", "build", "-R", "unit"],
    },
    DocumentedCommand {
        page: "containers.md",
        line: "cx -- docker ps",
        argv: &["cx", "--", "docker", "ps"],
    },
    DocumentedCommand {
        page: "find.md",
        line: "cx -- find src -maxdepth 2 -type f -name '*.rs'",
        argv: &[
            "cx",
            "--",
            "find",
            "src",
            "-maxdepth",
            "2",
            "-type",
            "f",
            "-name",
            "*.rs",
        ],
    },
    DocumentedCommand {
        page: "git.md",
        line: "cx git evidence-diff HEAD~1..HEAD -- src/lib.rs",
        argv: &[
            "cx",
            "git",
            "evidence-diff",
            "HEAD~1..HEAD",
            "--",
            "src/lib.rs",
        ],
    },
    DocumentedCommand {
        page: "go.md",
        line: "cx go test ./internal/api -run TestAuth",
        argv: &["cx", "go", "test", "./internal/api", "-run", "TestAuth"],
    },
    DocumentedCommand {
        page: "grep.md",
        line: "cx grep -e '--flag-name' src",
        argv: &["cx", "grep", "-e", "--flag-name", "src"],
    },
    DocumentedCommand {
        page: "insights.md",
        line: "cx insights export --format json --limit 25",
        argv: &[
            "cx", "insights", "export", "--format", "json", "--limit", "25",
        ],
    },
    DocumentedCommand {
        page: "insights.md",
        line: "cx insights dashboard --limit 25",
        argv: &["cx", "insights", "dashboard", "--limit", "25"],
    },
    DocumentedCommand {
        page: "insights.md",
        line: "cx insights routing --limit 20",
        argv: &["cx", "insights", "routing", "--limit", "20"],
    },
    DocumentedCommand {
        page: "insights.md",
        line: "cx insights report-triage --format json --limit 25",
        argv: &[
            "cx",
            "insights",
            "report-triage",
            "--format",
            "json",
            "--limit",
            "25",
        ],
    },
    DocumentedCommand {
        page: "ls.md",
        line: "cx -- ls -la docs/features",
        argv: &["cx", "--", "ls", "-la", "docs/features"],
    },
    DocumentedCommand {
        page: "node.md",
        line: "cx node --check a.js b.mjs c.cjs",
        argv: &["cx", "node", "--check", "a.js", "b.mjs", "c.cjs"],
    },
    DocumentedCommand {
        page: "parser.md",
        line: "cx ctest -N -R unit",
        argv: &["cx", "ctest", "-N", "-R", "unit"],
    },
    DocumentedCommand {
        page: "pytest.md",
        line: "cx pytest -q tests/test_api.py -k auth",
        argv: &["cx", "pytest", "-q", "tests/test_api.py", "-k", "auth"],
    },
    DocumentedCommand {
        page: "processes.md",
        line: "cx -- ps -axo pid,ppid,etime,command",
        argv: &["cx", "--", "ps", "-axo", "pid,ppid,etime,command"],
    },
    DocumentedCommand {
        page: "read-like.md",
        line: "cx -- sed -n '120,180p' src/lib.rs",
        argv: &["cx", "--", "sed", "-n", "120,180p", "src/lib.rs"],
    },
    DocumentedCommand {
        page: "read.md",
        line: "cx read src/lib.rs --range 120:180 --line-numbers",
        argv: &[
            "cx",
            "read",
            "src/lib.rs",
            "--range",
            "120:180",
            "--line-numbers",
        ],
    },
    DocumentedCommand {
        page: "shell.md",
        line: "cx -- bash -lc 'git status --short | wc -l'",
        argv: &["cx", "--", "bash", "-lc", "git status --short | wc -l"],
    },
    DocumentedCommand {
        page: "tsc.md",
        line: "cx tsc -p tsconfig.json --noEmit",
        argv: &["cx", "tsc", "-p", "tsconfig.json", "--noEmit"],
    },
];

fn feature_docs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/features")
}

#[test]
fn documented_command_contracts_are_present_and_parse() {
    let root = feature_docs();
    for example in DOCUMENTED_COMMANDS {
        let page = root.join(example.page);
        let content = fs::read_to_string(&page).unwrap();
        assert!(
            content.lines().any(|line| line == example.line),
            "{} no longer contains documented parser contract `{}`",
            page.display(),
            example.line,
        );
        try_parse_from_cx_args(example.argv.iter().copied()).unwrap_or_else(|error| {
            panic!(
                "{} documents an argv shape that CX cannot parse: {:?}: {error}",
                page.display(),
                example.argv,
            )
        });
    }
}

#[test]
fn feature_index_links_every_markdown_page() {
    let root = feature_docs();
    let index = fs::read_to_string(root.join("index.md")).unwrap();
    let mut pages = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
        .filter(|path| path.file_name().and_then(|value| value.to_str()) != Some("index.md"))
        .collect::<Vec<_>>();
    pages.sort();

    for page in pages {
        let name = page.file_name().unwrap().to_string_lossy();
        assert!(
            index.contains(&format!("({name})")),
            "{} is not linked from {}",
            page.display(),
            root.join("index.md").display(),
        );
        assert_feature_heading(&page);
    }
}

fn assert_feature_heading(path: &Path) {
    let content = fs::read_to_string(path).unwrap();
    assert!(
        content.starts_with("# CX "),
        "{} must start with a CX feature heading",
        path.display(),
    );
}
