use super::*;

#[cfg(unix)]
#[test]
fn run_passes_extended_regexp_flag_to_grep_fallback() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("grep-args.txt");
    let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/CMakeLists.txt:7:add_library(shell)\\n'\n",
            args_file.display()
        );
    crate::support::test_support::with_fake_path_only(&[("grep", &script)], || {
        let output = run(
            "lib/shell_dev|add_library\\(",
            &[String::from("src/CMakeLists.txt")],
            &GrepOptions {
                extended_regexp: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("add_library(shell)"));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.lines().any(|arg| arg == "-E"));
    assert!(args
        .lines()
        .any(|arg| arg == "lib/shell_dev|add_library\\("));
}

#[cfg(unix)]
#[test]
fn run_translates_grep_escaped_alternation_for_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:foo matched\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run("foo\\|bar", &[String::from("src")], &GrepOptions::default()).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, "src/main.rs:7:foo matched");
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.lines().any(|arg| arg == "foo|bar"));
    assert!(!args.lines().any(|arg| arg == "foo\\|bar"));
}

#[cfg(unix)]
#[test]
fn run_escapes_literal_bre_parentheses_for_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:ToJson(const VoiceConnectorSnapshot matched\\n'\n",
            args_file.display()
        );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "ToJson(const VoiceConnectorSnapshot",
            &[String::from("src")],
            &GrepOptions::default(),
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output
            .stdout
            .contains("ToJson(const VoiceConnectorSnapshot matched"));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args
        .lines()
        .any(|arg| arg == "ToJson\\(const VoiceConnectorSnapshot"));
}

#[cfg(unix)]
#[test]
fn run_preserves_unmatched_escaped_open_parens_for_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/CMakeLists.txt:7:add_library(found)\\n'\n",
            args_file.display()
        );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "lib/shell_dev|add_library\\(|target_sources\\(",
            &[String::from("src/CMakeLists.txt")],
            &GrepOptions::default(),
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("add_library(found)"));
        assert_eq!(
            output.observation.unwrap().source,
            "search backend=rg route=preferred dialect=basic mode=matches result=success hint=none"
        );
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args
        .lines()
        .any(|arg| arg == "lib/shell_dev\\|add_library\\(\\|target_sources\\("));
}

#[cfg(unix)]
#[test]
fn run_does_not_translate_fixed_string_alternation_for_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:foo|bar literal\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "foo\\|bar",
            &[String::from("src")],
            &GrepOptions {
                fixed_strings: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("foo|bar literal"));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.lines().any(|arg| arg == "foo\\|bar"));
}

#[cfg(unix)]
#[test]
fn run_preserves_grep_escaped_alternation_for_grep_fallback() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("grep-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:foo matched\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path_only(&[("grep", &script)], || {
        let output = run("foo\\|bar", &[String::from("src")], &GrepOptions::default()).unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("foo matched"));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.lines().any(|arg| arg == "foo\\|bar"));
    assert!(!args.lines().any(|arg| arg == "foo|bar"));
}

#[cfg(unix)]
#[test]
fn run_many_passes_multiple_patterns_to_grep_fallback() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("grep-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:template match\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path_only(&[("grep", &script)], || {
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
    let lines: Vec<_> = args.lines().collect();
    let pattern_args: Vec<_> = lines
        .windows(2)
        .filter_map(|window| (window[0] == "-e").then_some(window[1]))
        .collect();
    assert_eq!(pattern_args, vec!["`", "${"]);
}

#[cfg(unix)]
#[test]
fn run_retries_with_grep_on_rg_regex_parse_error() {
    let temp = tempdir().unwrap();
    let grep_args_file = temp.path().join("grep-args.txt");
    let rg_script = "#!/bin/sh\nprintf 'rg: regex parse error:\\n    (?:foo\\\\k)\\n           ^\\nerror: invalid escape sequence\\n' >&2\nexit 2\n";
    let grep_script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:fallback matched\\n'\n",
        grep_args_file.display()
    );
    crate::support::test_support::with_fake_path(
        &[("rg", rg_script), ("grep", &grep_script)],
        || {
            let output = run("foo\\k", &[String::from("src")], &GrepOptions::default()).unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("fallback matched"));
            assert_eq!(
                output.observation.unwrap().source,
                "search backend=grep route=rg-regex-retry dialect=basic mode=matches result=success hint=none"
            );
        },
    );

    let args = fs::read_to_string(grep_args_file).unwrap();
    assert!(args.lines().any(|arg| arg == "foo\\k"));
}

#[cfg(unix)]
#[test]
fn run_passes_archive_derived_search_flags_to_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:needle here\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "needle",
            &[String::from("src")],
            &GrepOptions {
                smart_case: true,
                context_lines: Some(2),
                hidden: true,
                text: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains("-S"));
    assert!(args.contains("-C"));
    assert!(args.contains("2"));
    assert!(args.contains("--hidden"));
    assert!(args.contains("-a"));
}

#[cfg(unix)]
#[test]
fn run_passes_only_matching_flag_to_rg() {
    let temp = tempdir().unwrap();
    let args_file = temp.path().join("rg-args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'src/main.rs:7:needle\\n'\n",
        args_file.display()
    );
    crate::support::test_support::with_fake_path(&[("rg", &script)], || {
        let output = run(
            "needle",
            &[String::from("src/main.rs")],
            &GrepOptions {
                only_matching: true,
                ..GrepOptions::default()
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("needle"));
    });

    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains("-o"));
}
