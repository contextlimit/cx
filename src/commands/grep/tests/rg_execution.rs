use super::*;

#[cfg(unix)]
#[test]
fn run_uses_fake_rg_from_path() {
    crate::support::test_support::with_fake_path(
        &[("rg", "#!/bin/sh\nprintf 'src/main.rs:7:needle here\\n'\n")],
        || {
            let output = run("needle", &[String::from(".")], &GrepOptions::default()).unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(output.stdout, "src/main.rs:7:needle here");
        },
    );
}

#[cfg(unix)]
#[test]
fn run_no_matches_warns_for_basic_unescaped_alternation() {
    crate::support::test_support::with_fake_path(&[("rg", "#!/bin/sh\nexit 1\n")], || {
        let output = run("foo|bar", &[String::from("src")], &GrepOptions::default()).unwrap();
        assert_eq!(output.exit_code, 1);
        assert!(output.stdout.contains("0 matches for 'foo|bar'"));
        assert!(output.stdout.contains("use `cx grep -E` or `cx rg`"));
        assert_eq!(
            output.observation.unwrap().source,
            "search backend=rg route=preferred dialect=basic mode=matches result=no-match hint=extended-alternation"
        );
    });
}

#[cfg(unix)]
#[test]
fn run_stores_failure_artifact_for_rg_errors() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "rg",
        "#!/bin/sh\nprintf 'regex parse error: unclosed group\\n' >&2\nexit 2\n",
    );

    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(bin.to_string_lossy().as_ref())),
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let output = run("(", &[String::from(".")], &GrepOptions::default()).unwrap();
            assert_eq!(output.exit_code, 2);
            assert!(output.stderr.contains("regex parse error"));
            assert!(output
                .stdout
                .contains("[full output: ~/.cx/cache/failures/grep/"));
            let artifact_dir = home.join(".cx/cache/failures/grep");
            assert_eq!(fs::read_dir(artifact_dir).unwrap().count(), 1);
        },
    );
}

#[cfg(unix)]
#[test]
fn run_rg_capture_returns_without_waiting_for_descendant_stdout_to_close() {
    crate::support::test_support::with_fake_path(
        &[(
            "rg",
            "#!/bin/sh\n(sleep 1) &\nprintf 'src/main.rs:7:needle here\\n'\nexit 0\n",
        )],
        || {
            let start = Instant::now();
            let output = run("needle", &[String::from(".")], &GrepOptions::default()).unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("needle here"));
            assert!(start.elapsed() < Duration::from_millis(700));
        },
    );
}

#[cfg(unix)]
#[test]
fn run_many_passes_multiple_patterns_to_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:template match\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run_many(
            &["`".to_string(), "${".to_string()],
            &[String::from("src")],
            &GrepOptions {
                fixed_strings: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("template match"));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.lines().any(|arg| arg == "-e"));
    assert!(args.lines().any(|arg| arg == "`"));
    assert!(args.contains("-F"));
    assert!(args.lines().any(|arg| arg == "${"));
}

#[cfg(unix)]
#[test]
fn run_accepts_multiple_paths() {
    crate::support::test_support::with_fake_path(
        &[(
            "rg",
            "#!/bin/sh\nprintf 'src/a.rs:3:needle here\\nsrc/b.rs:9:needle there\\n'\n",
        )],
        || {
            let output = run(
                "needle",
                &[String::from("src/a.rs"), String::from("src/b.rs")],
                &GrepOptions::default(),
            )
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(
                output.stdout,
                "src/a.rs:3:needle here\nsrc/b.rs:9:needle there"
            );
        },
    );
}

#[cfg(unix)]
#[test]
fn run_truncates_huge_line_from_rg_output() {
    let script = format!(
        "#!/bin/sh\nprintf 'src/blob.mjs:8:{}\\n'\n",
        "A".repeat(output::MATCH_LINE_PREVIEW_CHARS + 120)
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run("blob", &[String::from(".")], &GrepOptions::default()).unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("[truncated]"));
        assert!(!output
            .stdout
            .contains(&"A".repeat(output::MATCH_LINE_PREVIEW_CHARS + 40)));
    });
}

#[cfg(unix)]
#[test]
fn run_no_compact_preserves_native_output_and_pattern_path_order() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let raw = (1..=40)
        .map(|line| format!("src/main.rs:{line}:needle_{line:02}\n"))
        .collect::<String>();
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\n",
        args_file.display(),
        raw.replace('\'', "'\\''"),
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "^needle",
            &[String::from("src/main.rs")],
            &GrepOptions {
                no_compact: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, raw);
        assert!(output
            .observation
            .as_ref()
            .is_some_and(|observation| observation.uses_preserved_stream_termination()));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(!args.lines().any(|arg| arg == "--no-compact"));
    let args = args.lines().collect::<Vec<_>>();
    let pattern_index = args.iter().position(|arg| *arg == "-e").unwrap();
    assert_eq!(args[pattern_index + 1], "^needle");
    assert_eq!(args.last(), Some(&"src/main.rs"));
}

#[cfg(unix)]
#[test]
fn run_no_compact_preserves_empty_no_match_stdout() {
    crate::support::test_support::with_fake_path(&[("rg", "#!/bin/sh\nexit 1\n")], || {
        let output = run(
            "missing",
            &[String::from("src")],
            &GrepOptions {
                no_compact: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 1);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    });
}

#[cfg(unix)]
#[test]
fn run_no_compact_preserves_generated_long_lines() {
    let raw = format!(
        "src/blob.mjs:8:{}\n",
        "A".repeat(output::MATCH_LINE_PREVIEW_CHARS + 120)
    );
    let script = format!("#!/bin/sh\nprintf '%s' '{}'\n", raw.replace('\'', "'\\''"));
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "blob",
            &[String::from(".")],
            &GrepOptions {
                no_compact: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.stdout, raw);
        assert!(!output.stdout.contains("[truncated]"));
    });
}

#[cfg(unix)]
#[test]
fn run_truncates_huge_line_in_raw_context_fallback() {
    let script = format!(
        "#!/bin/sh\nprintf 'src/blob.mjs:8:{}\\n'\n",
        "B".repeat(output::MATCH_LINE_PREVIEW_CHARS + 120)
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "blob",
            &[String::from(".")],
            &GrepOptions {
                context_lines: Some(1),
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("[truncated]"));
        assert!(!output
            .stdout
            .contains(&"B".repeat(output::MATCH_LINE_PREVIEW_CHARS + 40)));
    });
}

#[cfg(unix)]
#[test]
fn run_defaults_to_current_directory_when_no_paths_are_given() {
    crate::support::test_support::with_fake_path(
        &[("rg", "#!/bin/sh\nprintf 'src/main.rs:7:needle here\\n'\n")],
        || {
            let output = run("needle", &[], &GrepOptions::default()).unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(output.stdout, "src/main.rs:7:needle here");
        },
    );
}

#[cfg(unix)]
#[test]
fn run_passes_fixed_string_and_glob_flags_to_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:needle here\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "literal.*value",
            &[String::from("src")],
            &GrepOptions {
                fixed_strings: true,
                no_ignore: true,
                glob_patterns: vec!["*.rs".to_string(), "src/**/*.toml".to_string()],
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains("-F"));
    assert!(args.contains("--no-ignore"));
    assert!(args.contains("-g"));
    assert!(args.contains("*.rs"));
    assert!(args.contains("src/**/*.toml"));
    assert!(args.contains("literal.*value"));
}
