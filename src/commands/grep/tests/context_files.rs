use super::*;

#[cfg(unix)]
#[test]
fn run_falls_back_to_raw_window_when_context_is_requested() {
    crate::support::test_support::with_fake_path(
            &[(
                "rg",
                "#!/bin/sh\ncat <<'EOF'\nsrc/main.rs-6-before\nsrc/main.rs:7:needle here\nsrc/main.rs-8-after\nEOF\n",
            )],
            || {
                let output = run(
                    "needle",
                    &[String::from("src")],
                    &GrepOptions {
                        context_lines: Some(1),
                        ..GrepOptions::default()
                    },
                )
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("src/main.rs:7:needle here"));
                assert!(output.stdout.contains("src/main.rs-6-before"));
            },
        );
}

#[cfg(unix)]
#[test]
fn list_files_uses_archive_style_flags() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/a.cpp\\nsrc/b.hpp\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = list_files(
            &[String::from("src"), String::from("external")],
            &GrepOptions {
                hidden: true,
                glob_patterns: vec!["*.cpp".to_string(), "*.hpp".to_string()],
                max_results: Some(1),
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("1 shown of 2 files"));
        assert!(output.stdout.contains("src/a.cpp"));
        assert_eq!(
            output.observation.unwrap().source,
            "search backend=rg route=preferred dialect=none mode=files result=success hint=none"
        );
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains("--files"));
    assert!(args.contains("--hidden"));
    assert!(args.contains("-g"));
    assert!(args.contains("*.cpp"));
    assert!(args.contains("*.hpp"));
}

#[cfg(unix)]
#[test]
fn list_files_capture_returns_without_waiting_for_descendant_stdout_to_close() {
    crate::support::test_support::with_fake_path(
        &[(
            "rg",
            "#!/bin/sh\n(sleep 1) &\nprintf 'src/a.rs\\nsrc/b.rs\\n'\nexit 0\n",
        )],
        || {
            let start = Instant::now();
            let output = list_files(&[String::from("src")], &GrepOptions::default()).unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("src/a.rs"));
            assert!(start.elapsed() < Duration::from_millis(700));
        },
    );
}

#[cfg(unix)]
#[test]
fn list_files_falls_back_without_rg() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("workspace");
    fs::create_dir_all(root.join("src/.hidden")).unwrap();
    fs::write(root.join("src/a.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/.hidden/b.rs"), "fn hidden() {}\n").unwrap();

    crate::support::test_support::with_fake_path_only(&[], || {
        let output = list_files(
            &[root.join("src").display().to_string()],
            &GrepOptions {
                glob_patterns: vec!["*.rs".to_string()],
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("a.rs"));
        assert!(!output.stdout.contains(".hidden/b.rs"));
        assert_eq!(
            output.observation.unwrap().source,
            "search backend=walkdir route=rg-unavailable dialect=none mode=files result=success hint=none"
        );
    });
}

#[cfg(unix)]
#[test]
fn run_formats_files_with_matches() {
    crate::support::test_support::with_fake_path(
        &[("rg", "#!/bin/sh\nprintf 'src/a.rs\\nsrc/b.rs\\n'\n")],
        || {
            let output = run(
                "needle",
                &[String::from("src")],
                &GrepOptions {
                    files_with_matches: true,
                    ..GrepOptions::default()
                },
            )
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(output.stdout, "src/a.rs\nsrc/b.rs\n");
        },
    );
}

#[cfg(unix)]
#[test]
fn run_preserves_large_files_with_matches_output_exactly() {
    let expected = (0..48)
        .map(|index| format!("src/file_{index:02}.rs\n"))
        .collect::<String>();
    let script = format!("#!/bin/sh\ncat <<'EOF'\n{expected}EOF\n");
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "needle",
            &[String::from("src")],
            &GrepOptions {
                files_with_matches: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, expected);
    });
}

#[cfg(unix)]
#[test]
fn run_preserves_empty_files_with_matches_output() {
    crate::support::test_support::with_fake_path(&[("rg", "#!/bin/sh\nexit 1\n")], || {
        let output = run(
            "needle",
            &[String::from("src")],
            &GrepOptions {
                files_with_matches: true,
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
fn run_falls_back_to_grep_when_rg_is_unavailable() {
    crate::support::test_support::with_fake_path_only(
        &[(
            "grep",
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
            assert_eq!(
                output.observation.unwrap().source,
                "search backend=grep route=rg-unavailable dialect=basic mode=matches result=success hint=none"
            );
        },
    );
}

#[cfg(unix)]
#[test]
fn grep_fallback_capture_returns_without_waiting_for_descendant_stdout_to_close() {
    crate::support::test_support::with_fake_path_only(
        &[(
            "grep",
            "#!/bin/sh\n(sleep 1) &\nprintf 'src/a.rs:3:needle here\\n'\nexit 0\n",
        )],
        || {
            let start = Instant::now();
            let output = run(
                "needle",
                &[String::from("src/a.rs")],
                &GrepOptions::default(),
            )
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("needle here"));
            assert!(start.elapsed() < Duration::from_millis(700));
        },
    );
}

#[cfg(unix)]
#[test]
fn grep_fallback_respects_glob_filters() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("workspace");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/config.json"), "{ \"needle\": true }\n").unwrap();
    fs::write(root.join("src/config.toml"), "needle = true\n").unwrap();
    let args_file = temp.path().join("grep-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '{}:1:needle\\n'\n",
        args_file.display(),
        root.join("src/config.json").display()
    );

    crate::support::test_support::with_fake_path_only(&[("grep", &script)], || {
        let output = run(
            "needle",
            &[root.join("src").display().to_string()],
            &GrepOptions {
                glob_patterns: vec!["*.json".to_string()],
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("config.json"));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains("config.json"));
    assert!(!args.contains("config.toml"));
}
