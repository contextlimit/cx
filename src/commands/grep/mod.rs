mod exec;
mod output;
mod pattern;
pub(crate) mod reporting;

use anyhow::Result;

use crate::support::insights;
use crate::support::runner::{
    append_failure_hint, capture_stdin_if_present, CommandOutput, ProxyOutcome,
};
use crate::support::utils::{fallback_window, tool_exists};

use exec::{
    collect_files_fallback, run_grep_fallback, run_rg, run_rg_files,
    should_retry_with_grep_fallback,
};
use output::{
    basic_alternation_hint, display_patterns, format_file_list, format_files_with_matches,
    format_matches, no_matches_outcome, output_is_document_only, requires_raw_match_output,
    truncate_output_lines,
};

const SMALL_EXACT_RESULT_LINES: usize = 8;
use pattern::normalize_ripgrep_patterns;
use reporting::{SearchBackend, SearchRoute};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GrepOptions {
    pub extended_regexp: bool,
    pub ignore_case: bool,
    pub smart_case: bool,
    pub context_before: Option<usize>,
    pub context_after: Option<usize>,
    pub context_lines: Option<usize>,
    pub files_with_matches: bool,
    pub hidden: bool,
    pub no_ignore: bool,
    pub text: bool,
    pub only_matching: bool,
    pub fixed_strings: bool,
    pub glob_patterns: Vec<String>,
    pub max_results: Option<usize>,
    pub no_compact: bool,
}

pub fn run(pattern: &str, paths: &[String], options: &GrepOptions) -> Result<ProxyOutcome> {
    run_many(&[pattern.to_string()], paths, options)
}

pub fn run_many(
    patterns: &[String],
    paths: &[String],
    options: &GrepOptions,
) -> Result<ProxyOutcome> {
    let captured_stdin = if paths.is_empty() {
        capture_stdin_if_present()?
    } else {
        None
    };
    let effective_paths = if captured_stdin.is_some() {
        vec!["-".to_string()]
    } else if paths.is_empty() {
        vec![".".to_string()]
    } else {
        paths.to_vec()
    };
    let effective_patterns = if patterns.is_empty() {
        vec![String::new()]
    } else {
        patterns.to_vec()
    };
    let ripgrep_patterns = normalize_ripgrep_patterns(&effective_patterns, options);
    let pattern_display = display_patterns(&effective_patterns);

    let (mut output, backend, route) = if tool_exists("rg") {
        let output = run_rg(
            &ripgrep_patterns,
            &effective_paths,
            options,
            captured_stdin.as_ref(),
        )?;
        if should_retry_with_grep_fallback(&output, options) && tool_exists("grep") {
            (
                run_grep_fallback(
                    &effective_patterns,
                    &effective_paths,
                    options,
                    captured_stdin.as_ref(),
                )?,
                SearchBackend::Grep,
                SearchRoute::RgRegexRetry,
            )
        } else {
            (output, SearchBackend::Rg, SearchRoute::Preferred)
        }
    } else {
        (
            run_grep_fallback(
                &effective_patterns,
                &effective_paths,
                options,
                captured_stdin.as_ref(),
            )?,
            SearchBackend::Grep,
            SearchRoute::RgUnavailable,
        )
    };

    let exit_code = output.exit_code;
    let failure_hint = if exit_code > 1 {
        output.failure_artifact_hint("grep")
    } else {
        None
    };
    let alternation_hint = basic_alternation_hint(&effective_patterns, options);
    let observation =
        search_observation(backend, route, options, &output, alternation_hint.is_some());

    if options.no_compact {
        return Ok(exact_search_outcome(output, observation, failure_hint));
    }

    if let Some(outcome) = exact_files_with_matches_outcome(options, &mut output, &observation) {
        return Ok(outcome);
    }

    if output.stdout.trim().is_empty() {
        let mut outcome = no_matches_outcome(
            &pattern_display,
            &output.stderr,
            exit_code,
            alternation_hint,
        );
        outcome.stdout = append_failure_hint(outcome.stdout, failure_hint.as_deref());
        return Ok(outcome
            .with_observation(observation)
            .with_expansion_reason("no-match-summary"));
    }

    let formatted = format_stdout(
        &pattern_display,
        &effective_paths,
        &output.stdout,
        options,
        insights::compact_document_search_results_enabled()?,
    );
    Ok(ProxyOutcome {
        stdout: append_failure_hint(formatted, failure_hint.as_deref()),
        stderr: output.stderr.trim_end().to_string(),
        exit_code,
        observation: None,
    }
    .with_observation(observation)
    .with_expansion_reason("search-result-formatting"))
}

fn search_observation(
    backend: SearchBackend,
    route: SearchRoute,
    options: &GrepOptions,
    output: &CommandOutput,
    alternation_hint_available: bool,
) -> insights::OutputObservation {
    let has_results = !output.stdout.trim().is_empty();
    let source = reporting::observation_source(
        backend,
        route,
        options,
        false,
        output.exit_code,
        has_results,
        !has_results && alternation_hint_available,
    );
    output.observation(source)
}

fn exact_files_with_matches_outcome(
    options: &GrepOptions,
    output: &mut CommandOutput,
    observation: &insights::OutputObservation,
) -> Option<ProxyOutcome> {
    (options.files_with_matches && options.max_results.is_none() && output.exit_code <= 1).then(
        || {
            ProxyOutcome {
                stdout: std::mem::take(&mut output.stdout),
                stderr: std::mem::take(&mut output.stderr),
                exit_code: output.exit_code,
                observation: None,
            }
            .with_observation(observation.clone().with_preserved_stream_termination())
        },
    )
}

fn format_stdout(
    pattern_display: &str,
    effective_paths: &[String],
    stdout: &str,
    options: &GrepOptions,
    compact_document_search_results: bool,
) -> String {
    let preserve_protected_results = output::output_is_compaction_protected_only(
        effective_paths,
        stdout,
        options.files_with_matches,
    );
    let document_only =
        output_is_document_only(effective_paths, stdout, options.files_with_matches);
    let preserve_document_results = !compact_document_search_results && document_only;
    if preserve_protected_results || preserve_document_results {
        return stdout.trim_end().to_string();
    }
    if !(compact_document_search_results && document_only) {
        if let Some(exact) = small_exact_search_output(stdout, options) {
            return exact;
        }
    }
    if options.files_with_matches {
        return format_files_with_matches(pattern_display, stdout, options.max_results);
    }
    if requires_raw_match_output(options) {
        return fallback_window(&truncate_output_lines(stdout), 12, 28);
    }
    format_matches(pattern_display, effective_paths, stdout, options)
        .unwrap_or_else(|| fallback_window(&truncate_output_lines(stdout), 12, 28))
}

fn small_exact_search_output(stdout: &str, options: &GrepOptions) -> Option<String> {
    if options.max_results.is_some() {
        return None;
    }
    let result_lines = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    (1..=SMALL_EXACT_RESULT_LINES)
        .contains(&result_lines)
        .then(|| truncate_output_lines(stdout))
}

pub fn list_files(paths: &[String], options: &GrepOptions) -> Result<ProxyOutcome> {
    let effective_paths = if paths.is_empty() {
        vec![".".to_string()]
    } else {
        paths.to_vec()
    };

    let (stdout, raw_output, backend, route) = if tool_exists("rg") {
        let mut output = run_rg_files(&effective_paths, options)?;
        let stderr = output.stderr.trim_end().to_string();
        let exit_code = output.exit_code;
        if exit_code != 0 {
            let failure_hint = output.failure_artifact_hint("grep");
            let source = reporting::observation_source(
                SearchBackend::Rg,
                SearchRoute::Preferred,
                options,
                true,
                exit_code,
                !output.stdout.trim().is_empty(),
                false,
            );
            let observation = output.observation(source);
            if options.no_compact {
                return Ok(exact_search_outcome(output, observation, failure_hint));
            }
            return Ok(ProxyOutcome {
                stdout: append_failure_hint(
                    fallback_window(&output.stdout, 12, 28),
                    failure_hint.as_deref(),
                ),
                stderr,
                exit_code,
                observation: None,
            }
            .with_observation(observation));
        }
        let raw_output = output.combined.clone();
        (
            output.stdout,
            raw_output,
            SearchBackend::Rg,
            SearchRoute::Preferred,
        )
    } else {
        let collected = collect_files_fallback(&effective_paths, options)?.join("\n");
        (
            collected.clone(),
            collected,
            SearchBackend::Walkdir,
            SearchRoute::RgUnavailable,
        )
    };
    let source = reporting::observation_source(
        backend,
        route,
        options,
        true,
        0,
        !stdout.trim().is_empty(),
        false,
    );
    let observation = insights::OutputObservation::from_text(source.clone(), &raw_output);
    if options.no_compact {
        return Ok(ProxyOutcome {
            stdout,
            stderr: String::new(),
            exit_code: 0,
            observation: None,
        }
        .with_observation(observation.with_preserved_stream_termination()));
    }

    Ok(ProxyOutcome {
        stdout: format_file_list(&stdout, options.max_results),
        stderr: String::new(),
        exit_code: 0,
        observation: None,
    }
    .with_raw_output(source, &raw_output)
    .with_expansion_reason("file-list-summary"))
}

fn exact_search_outcome(
    mut output: CommandOutput,
    observation: insights::OutputObservation,
    failure_hint: Option<String>,
) -> ProxyOutcome {
    ProxyOutcome {
        stdout: append_failure_hint(std::mem::take(&mut output.stdout), failure_hint.as_deref()),
        stderr: std::mem::take(&mut output.stderr),
        exit_code: output.exit_code,
        observation: None,
    }
    .with_observation(observation.with_preserved_stream_termination())
}

#[cfg(test)]
mod tests;
