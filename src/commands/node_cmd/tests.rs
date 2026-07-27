use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn parse_check_args_requires_check_flag() {
    let error = parse_check_args(&["widget.jsx".to_string()]).unwrap_err();
    assert!(error.to_string().contains("only supports `--check <file>`"));
}

#[test]
fn parse_check_args_accepts_short_flag() {
    let parsed = parse_check_args(&["-c".to_string(), "widget.jsx".to_string()]).unwrap();
    assert_eq!(parsed.files, vec![PathBuf::from("widget.jsx")]);
}

#[test]
fn parse_check_args_accepts_multiple_files() {
    let parsed = parse_check_args(&[
        "--check".to_string(),
        "widget.jsx".to_string(),
        "chat.jsx".to_string(),
        "chat.js".to_string(),
    ])
    .unwrap();
    assert_eq!(
        parsed.files,
        vec![
            PathBuf::from("widget.jsx"),
            PathBuf::from("chat.jsx"),
            PathBuf::from("chat.js"),
        ]
    );
}

#[test]
fn parse_check_args_rejects_check_flag_after_program_path() {
    let error = parse_check_args(&["script.mjs".to_string(), "--check".to_string()]).unwrap_err();
    assert!(error.to_string().contains("must precede file paths"));
    assert!(error.to_string().contains("`cx -- node <script> --check`"));
}

#[test]
fn parse_check_args_accepts_hyphenated_file_after_separator() {
    let parsed = parse_check_args(&[
        "--check".to_string(),
        "--".to_string(),
        "-leading-name.jsx".to_string(),
        "normal.js".to_string(),
    ])
    .unwrap();
    assert_eq!(
        parsed.files,
        vec![
            PathBuf::from("-leading-name.jsx"),
            PathBuf::from("normal.js")
        ]
    );
}

#[test]
fn parse_check_args_ignores_loader_flags() {
    let parsed = parse_check_args(&[
        "--experimental-loader".to_string(),
        "./jsx_loader.mjs".to_string(),
        "--check".to_string(),
        "widget.jsx".to_string(),
        "--loader=./other_loader.mjs".to_string(),
        "plain.js".to_string(),
    ])
    .unwrap();
    assert_eq!(
        parsed.files,
        vec![PathBuf::from("widget.jsx"), PathBuf::from("plain.js")]
    );
}

#[test]
fn parse_check_args_requires_loader_path() {
    let error = parse_check_args(&[
        "--check".to_string(),
        "widget.jsx".to_string(),
        "--loader".to_string(),
    ])
    .unwrap_err();
    assert!(error.to_string().contains("requires a loader path"));
}

#[test]
fn parse_check_args_rejects_extra_flags() {
    let error =
        parse_check_args(&["--check".to_string(), "--input-type=module".to_string()]).unwrap_err();
    assert!(error.to_string().contains("unsupported argument"));
}

#[test]
fn parse_node_invocation_accepts_runtime_modes() {
    assert!(matches!(
        parse_node_invocation(&[]).unwrap(),
        NodeInvocation::Run(args) if args.is_empty()
    ));
    assert!(matches!(
        parse_node_invocation(&strings(&["run", "script.mjs"])).unwrap(),
        NodeInvocation::Run(args) if args == vec!["script.mjs".to_string()]
    ));
    assert!(matches!(
        parse_node_invocation(&strings(&["test", "script.test.mjs"])).unwrap(),
        NodeInvocation::Test(args) if args == vec!["script.test.mjs".to_string()]
    ));
    assert!(matches!(
        parse_node_invocation(&strings(&["--input-type=module"])).unwrap(),
        NodeInvocation::Run(args) if args == vec!["--input-type=module".to_string()]
    ));
    assert!(matches!(
        parse_node_invocation(&strings(&["--input-type", "module"])).unwrap(),
        NodeInvocation::Run(args) if args == vec!["--input-type".to_string(), "module".to_string()]
    ));
}

#[test]
fn parse_node_invocation_requires_run_args() {
    let error = parse_node_invocation(&strings(&["run"])).unwrap_err();
    assert!(error
        .to_string()
        .contains("`cx node run` requires node arguments"));
}

#[test]
fn command_label_separates_node_modes() {
    assert_eq!(command_label(&[]), "node run");
    assert_eq!(
        command_label(&strings(&["--check", "script.js"])),
        "node check"
    );
    assert_eq!(command_label(&strings(&["run", "script.mjs"])), "node run");
    assert_eq!(
        command_label(&strings(&["test", "script.test.mjs"])),
        "node test"
    );
    assert_eq!(
        command_label(&strings(&["--input-type=module"])),
        "node run"
    );
    assert_eq!(
        command_label(&strings(&["script.mjs", "--check"])),
        "node run"
    );
    assert!(check_flag_precedes_program(&strings(&[
        "--experimental-loader",
        "./loader.mjs",
        "--check",
        "widget.jsx",
    ])));
    assert!(!check_flag_precedes_program(&strings(&[
        "script.mjs",
        "--check",
    ])));
}

#[test]
fn run_checks_jsx_files_without_node() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("widget.jsx");
    fs::write(
        &file,
        "export default function Widget() { return <div />; }\n",
    )
    .unwrap();

    let output = run(&["--check".to_string(), file.display().to_string()]).unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout.contains("syntax ok"));
    assert!(output.stdout.contains("[jsx parser]"));
    assert!(output.stderr.is_empty());
}

#[test]
fn run_checks_jsx_files_with_loader_flag_without_node() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("widget.jsx");
    fs::write(
        &file,
        "export default function Widget() { return <div data-ok />; }\n",
    )
    .unwrap();

    let output = run(&[
        "--experimental-loader".to_string(),
        "./jsx_loader.mjs".to_string(),
        "--check".to_string(),
        file.display().to_string(),
    ])
    .unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout.contains("syntax ok"));
    assert!(output.stdout.contains("[jsx parser]"));
    assert!(output.stderr.is_empty());
}

#[test]
fn run_checks_multiple_jsx_files() {
    let temp = tempdir().unwrap();
    let first = temp.path().join("widget.jsx");
    let second = temp.path().join("chat.jsx");
    fs::write(
        &first,
        "export default function Widget() { return <div />; }\n",
    )
    .unwrap();
    fs::write(&second, "export const Chat = () => <section />;\n").unwrap();

    let output = run(&[
        "--check".to_string(),
        first.display().to_string(),
        second.display().to_string(),
    ])
    .unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout.contains(&first.display().to_string()));
    assert!(output.stdout.contains(&second.display().to_string()));
    assert!(output.stderr.is_empty());
}

#[test]
fn run_reports_jsx_syntax_errors() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("broken.jsx");
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        &file,
        "export default function Widget() { return <div>; }\n",
    )
    .unwrap();

    crate::support::test_support::with_env_vars(
        &[("HOME", Some(home.to_string_lossy().as_ref()))],
        || {
            let output = run(&["--check".to_string(), file.display().to_string()]).unwrap();
            assert_eq!(output.exit_code, 1);
            assert!(output.stderr.contains("broken.jsx"));
            assert!(output.stderr.contains(":1:"));
            assert!(output
                .stderr
                .contains("[full output: ~/.cx/cache/failures/node/"));
            let artifact_dir = home.join(".cx/cache/failures/node");
            let artifacts = fs::read_dir(artifact_dir)
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(artifacts.len(), 1);
            let artifact_body = fs::read_to_string(artifacts[0].path()).unwrap();
            assert!(artifact_body.contains("broken.jsx"));
            assert!(artifact_body.contains("[stderr]"));
        },
    );
}

#[cfg(unix)]
#[test]
fn run_delegates_js_files_to_node() {
    crate::support::test_support::with_fake_path(
            &[(
                "node",
                "#!/bin/sh\nif [ \"$1\" = \"--check\" ] && [ \"$2\" = \"widget.js\" ]; then\nexit 0\nfi\nprintf 'unexpected invocation\\n' >&2\nexit 7\n",
            )],
            || {
                let output = run(&["--check".to_string(), "widget.js".to_string()]).unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("widget.js"));
            },
        );
}

#[cfg(unix)]
#[test]
fn run_checks_multiple_js_files_with_node() {
    crate::support::test_support::with_fake_path(
            &[(
                "node",
                "#!/bin/sh\nif [ \"$1\" != \"--check\" ]; then\nprintf 'unexpected flag\\n' >&2\nexit 7\nfi\ncase \"$2\" in\n  widget.js|chat.js) exit 0 ;;\n  *) printf 'unexpected file %s\\n' \"$2\" >&2; exit 8 ;;\nesac\n",
            )],
            || {
                let output = run(&[
                    "--check".to_string(),
                    "widget.js".to_string(),
                    "chat.js".to_string(),
                ])
                .unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("widget.js"));
                assert!(output.stdout.contains("chat.js"));
                assert!(output.stderr.is_empty());
            },
        );
}

#[cfg(unix)]
#[test]
fn run_reports_all_failures_across_files() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "node",
            "#!/bin/sh\nif [ \"$1\" != \"--check\" ]; then\nprintf 'unexpected flag\\n' >&2\nexit 7\nfi\nif [ \"$2\" = \"good.js\" ]; then\nexit 0\nfi\nprintf '%s: SyntaxError: missing ) after argument list\\n' \"$2\" >&2\nexit 1\n",
        );

    crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(bin.to_string_lossy().as_ref())),
            ("HOME", Some(home.to_string_lossy().as_ref())),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            let output = run(&[
                "--check".to_string(),
                "good.js".to_string(),
                "bad.js".to_string(),
                "worse.js".to_string(),
            ])
            .unwrap();
            assert_eq!(output.exit_code, 1);
            assert!(output.stdout.contains("good.js"));
            assert!(output.stderr.contains("bad.js"));
            assert!(output.stderr.contains("worse.js"));
            assert!(output
                .stderr
                .contains("[full output: ~/.cx/cache/failures/node/"));
            let artifact_dir = home.join(".cx/cache/failures/node");
            assert_eq!(fs::read_dir(artifact_dir).unwrap().count(), 2);
        },
    );
}

#[cfg(unix)]
#[test]
fn run_node_runtime_preserves_arbitrary_long_stdout() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "node",
            "#!/bin/sh\nif [ \"$1\" = \"-e\" ] && [ \"$2\" = \"many\" ]; then\ni=1\nwhile [ \"$i\" -le 80 ]; do printf 'line_%02d\\n' \"$i\"; i=$((i + 1)); done\nexit 0\nfi\nprintf 'unexpected invocation\\n' >&2\nexit 9\n",
        );

    let path = bin.to_string_lossy().to_string();
    let home_string = home.to_string_lossy().to_string();
    let outcome = crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_string.as_str())),
            ("CX_DISABLE_INSIGHTS", Some("1")),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || run(&strings(&["run", "-e", "many"])).unwrap(),
    );

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.contains("line_01"));
    assert!(outcome.stdout.contains("line_20"));
    assert!(outcome.stdout.contains("line_80"));
    assert!(!outcome.stdout.contains("lines omitted"));
    assert!(outcome.stderr.is_empty());
    assert!(outcome
        .observation
        .as_ref()
        .is_some_and(|raw| raw.source == "node run" && raw.metrics.lines == 80));
}

#[cfg(unix)]
#[test]
fn run_node_runtime_preserves_generated_and_source_lines() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "node",
        "#!/bin/sh\nif [ \"$2\" = generated ]; then\nprintf '{\"status\":\"ok\",\"payload\":\"'\ni=0; while [ \"$i\" -lt 4000 ]; do printf A; i=$((i + 1)); done\nprintf '\",\"tail\":\"done\"}\\n'\nexit 0\nfi\nif [ \"$2\" = source ]; then\nprintf 'const matcher = /'\ni=0; while [ \"$i\" -lt 80 ]; do printf 'route|path|dashboard|'; i=$((i + 1)); done\nprintf '/; return matcher.test(candidate);\\n'\nexit 0\nfi\nexit 9\n",
    );

    let path = bin.to_string_lossy().to_string();
    let home_string = home.to_string_lossy().to_string();
    let (generated, source) = crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_string.as_str())),
            ("CX_DISABLE_INSIGHTS", Some("1")),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || {
            (
                run(&strings(&["run", "-e", "generated"])).unwrap(),
                run(&strings(&["run", "-e", "source"])).unwrap(),
            )
        },
    );

    assert_eq!(generated.exit_code, 0);
    assert!(generated.stdout.starts_with(r#"{"status":"ok""#));
    assert!(generated.stdout.ends_with("\"tail\":\"done\"}\n"));
    assert!(!generated.stdout.contains("generated line truncated"));
    assert!(generated.stdout.len() > 4_000);

    assert_eq!(source.exit_code, 0);
    assert!(source
        .stdout
        .starts_with("const matcher = /route|path|dashboard|"));
    assert!(source
        .stdout
        .ends_with("/; return matcher.test(candidate);\n"));
    assert!(!source.stdout.contains("generated line truncated"));
    assert!(source.stdout.len() > 1_200);
}

#[cfg(unix)]
#[test]
fn run_node_test_maps_to_node_test_runner() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "node",
            "#!/bin/sh\nif [ \"$1\" = \"--test\" ] && [ \"$2\" = \"sample.test.mjs\" ]; then\nprintf 'TAP version 13\\n# sample ok\\n'\nexit 0\nfi\nprintf 'unexpected invocation: %s %s\\n' \"$1\" \"$2\" >&2\nexit 9\n",
        );

    let path = bin.to_string_lossy().to_string();
    let home_string = home.to_string_lossy().to_string();
    let outcome = crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_string.as_str())),
            ("CX_DISABLE_INSIGHTS", Some("1")),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || run(&strings(&["test", "sample.test.mjs"])).unwrap(),
    );

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.contains("TAP version 13"));
    assert!(outcome.stdout.contains("# sample ok"));
    assert!(outcome.stderr.is_empty());
    assert!(outcome
        .observation
        .as_ref()
        .is_some_and(|raw| raw.source == "node test"));
}

#[cfg(unix)]
#[test]
fn run_node_test_compacts_long_tap_output() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "node",
        "#!/bin/sh\nif [ \"$1\" != \"--test\" ]; then exit 9; fi\nprintf 'TAP version 13\\n'\ni=1\nwhile [ \"$i\" -le 100 ]; do printf 'ok %s - test %s\\n' \"$i\" \"$i\"; i=$((i + 1)); done\nprintf '1..100\\n# tests 100\\n# pass 100\\n# fail 0\\n# duration_ms 20\\n'\n",
    );

    let path = bin.to_string_lossy().to_string();
    let home_string = home.to_string_lossy().to_string();
    let outcome = crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_string.as_str())),
            ("CX_DISABLE_INSIGHTS", Some("1")),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || run(&strings(&["test", "suite.test.mjs"])).unwrap(),
    );

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.contains("node test: ok"));
    assert!(outcome.stdout.contains("TAP version 13"));
    assert!(outcome.stdout.contains("# tests 100"));
    assert!(outcome.stdout.contains("# fail 0"));
    assert!(outcome.stdout.contains("lines omitted"));
    assert!(!outcome.stdout.contains("ok 50 - test 50"));
}

#[cfg(unix)]
#[test]
fn run_node_test_failure_keeps_evidence_and_full_artifact() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
        &bin,
        "node",
        "#!/bin/sh\nprintf 'TAP version 13\\n'\ni=1\nwhile [ \"$i\" -le 100 ]; do\n  if [ \"$i\" -eq 57 ]; then\n    printf 'not ok 57 - contract failure\\n  error: expected true but received false\\n'\n  else\n    printf 'ok %s - test %s\\n' \"$i\" \"$i\"\n  fi\n  i=$((i + 1))\ndone\nprintf '1..100\\n# tests 100\\n# pass 99\\n# fail 1\\n'\nexit 1\n",
    );

    let path = bin.to_string_lossy().to_string();
    let home_string = home.to_string_lossy().to_string();
    let outcome = crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_string.as_str())),
            ("CX_DISABLE_INSIGHTS", Some("1")),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || run(&strings(&["test", "suite.test.mjs"])).unwrap(),
    );

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.contains("node test: failed (exit 1)"));
    assert!(outcome.stdout.contains("not ok 57 - contract failure"));
    assert!(outcome.stdout.contains("expected true but received false"));
    assert!(outcome
        .stdout
        .contains("[full output: ~/.cx/cache/failures/node/"));
    assert!(!outcome.stdout.contains("ok 40 - test 40"));

    let artifact_dir = home.join(".cx/cache/failures/node");
    let artifact = fs::read_dir(artifact_dir).unwrap().next().unwrap().unwrap();
    let body = fs::read_to_string(artifact.path()).unwrap();
    assert!(body.contains("ok 40 - test 40"));
    assert!(body.contains("not ok 57 - contract failure"));
}

#[cfg(unix)]
#[test]
fn run_node_input_type_executes_as_runtime() {
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    crate::support::test_support::write_executable(
            &bin,
            "node",
            "#!/bin/sh\nif [ \"$1\" = \"--input-type=module\" ]; then\nprintf 'stdin-runtime-ok\\n'\nexit 0\nfi\nprintf 'unexpected invocation: %s\\n' \"$*\" >&2\nexit 9\n",
        );

    let path = bin.to_string_lossy().to_string();
    let home_string = home.to_string_lossy().to_string();
    let outcome = crate::support::test_support::with_env_vars(
        &[
            ("PATH", Some(path.as_str())),
            ("HOME", Some(home_string.as_str())),
            ("CX_DISABLE_INSIGHTS", Some("1")),
            ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
            ("CX_TOOL_FALLBACK_PATHS", None),
        ],
        || run(&strings(&["--input-type=module"])).unwrap(),
    );

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, "stdin-runtime-ok\n");
    assert!(outcome.stderr.is_empty());
    assert!(outcome
        .observation
        .as_ref()
        .is_some_and(|raw| raw.source == "node run"));
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
