#![cfg(unix)]

use cx::commands::{grep, read_like};

#[allow(dead_code)]
#[path = "../benches/support/mod.rs"]
mod bench_support;

use bench_support as support;

#[test]
fn structured_rust_literal_metrics_preserve_exact_source() {
    let temp = support::ProjectTempDir::new("metrics-structured-rust-literal");
    let source_path = temp.path().join("src/archive_fixture.rs");
    let source_line = structured_source_line();
    let source = format!("before\n{source_line}\nafter\n");
    support::write_file(&source_path, &source);

    let outcome = read_like::run_sed(&[
        "-n".to_string(),
        "2,2p".to_string(),
        source_path.display().to_string(),
    ])
    .unwrap();

    assert_eq!(outcome.stdout, format!("{source_line}\n"));
    assert_eq!(outcome.stdout.len(), source_line.len() + 1);
    assert_eq!(outcome.stdout.lines().count(), 1);
    assert!(!outcome.stdout.contains("[truncated]"));
    assert!(outcome.stdout.contains("{raw_tokens}"));
    assert!(outcome.stdout.contains("{saved_tokens}"));
}

#[test]
fn cpp_initializer_range_metrics_preserve_exact_source() {
    let temp = support::ProjectTempDir::new("metrics-cpp-initializer");
    let source_path = temp.path().join("src/command_registry.cpp");
    let description = "Set true when the current user request requires a new or bound plan before repository work. With no bound plan, sync_context returns createRequiredPlan with shouldStop and mustBindBeforeWork; only plan preflight, allowed plan-authoring files, and plan persistence may proceed until a plan is bound. ";
    let source_line = format!(
        "              {{\"description\", \"{}\"}}}},",
        description.repeat(2)
    );
    support::write_file(&source_path, &format!("before\n{source_line}\nafter\n"));

    let outcome = read_like::run_sed(&[
        "-n".to_string(),
        "2,2p".to_string(),
        source_path.display().to_string(),
    ])
    .unwrap();

    assert_eq!(outcome.stdout, format!("{source_line}\n"));
    assert_eq!(outcome.stdout.len(), source_line.len() + 1);
    assert!(!outcome.stdout.contains("[truncated]"));
    assert!(outcome.stdout.contains("mustBindBeforeWork"));
}

#[test]
fn grep_metrics_preserve_long_structured_source_evidence() {
    let temp = support::ProjectTempDir::new("metrics-grep-structured-source");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let source_line = structured_source_line();
    let raw = format!("src/archive.rs:330:{source_line}\n");
    let output_path = temp.path().join("rg.out");
    support::write_file(&output_path, &raw);
    support::write_executable(
        &bin,
        "rg",
        &support::output_script(Some(&output_path), None, 0),
    );
    let _env = support::EnvGuard::fake_path(&bin, &home);

    let outcome = grep::run(
        "rawBytes",
        &["src/archive.rs".to_string()],
        &grep::GrepOptions {
            fixed_strings: true,
            ..grep::GrepOptions::default()
        },
    )
    .unwrap();

    assert!(outcome.stdout.contains(&source_line));
    assert!(!outcome.stdout.contains("[truncated]"));
    assert!(outcome.stdout.contains("{raw_tokens}"));
    assert!(outcome.stdout.contains("{saved_tokens}"));
    let source = outcome.observation.unwrap().source;
    assert!(source.contains("backend=rg"));
    assert!(source.contains("route=preferred"));
    assert!(source.contains("dialect=fixed"));
}

fn structured_source_line() -> String {
    concat!(
        "r#\"{{\"rawBytes\":{raw_bytes},\"rawChars\":{raw_chars},",
        "\"rawLines\":{raw_lines},\"rawTokens\":{raw_tokens},",
        "\"emittedBytes\":{emitted_bytes},\"emittedChars\":{emitted_chars},",
        "\"emittedLines\":{emitted_lines},\"emittedTokens\":{emitted_tokens},",
        "\"savedBytes\":{saved_bytes},\"savedChars\":{saved_chars},",
        "\"savedLines\":{saved_lines},\"savedTokens\":{saved_tokens}}}\"#,"
    )
    .to_string()
}
