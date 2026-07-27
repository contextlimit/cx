use std::collections::BTreeMap;

use crate::support::document_formats::{is_compaction_protected_path, is_document_path};
use crate::support::runner::ProxyOutcome;
use crate::support::source_lines::truncate_generated_line;

use super::GrepOptions;

pub(super) const MATCH_LINE_PREVIEW_CHARS: usize = 240;

pub(super) fn requires_raw_match_output(options: &GrepOptions) -> bool {
    options.context_before.is_some()
        || options.context_after.is_some()
        || options.context_lines.is_some()
}

pub(super) fn output_is_document_only(
    paths: &[String],
    stdout: &str,
    files_with_matches: bool,
) -> bool {
    output_paths_only(paths, stdout, files_with_matches, |path| {
        is_document_path(path)
    })
}

pub(super) fn output_is_compaction_protected_only(
    paths: &[String],
    stdout: &str,
    files_with_matches: bool,
) -> bool {
    output_paths_only(paths, stdout, files_with_matches, |path| {
        is_compaction_protected_path(path)
    })
}

fn output_paths_only(
    paths: &[String],
    stdout: &str,
    files_with_matches: bool,
    predicate: impl Fn(&str) -> bool,
) -> bool {
    let mut saw_path = false;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        if line == "--" {
            continue;
        }
        let path = if files_with_matches {
            Some(line.trim())
        } else {
            output_line_path(paths, line)
        };
        let Some(path) = path else {
            return false;
        };
        if !predicate(path) {
            return false;
        }
        saw_path = true;
    }
    saw_path
}

fn output_line_path<'a>(paths: &'a [String], line: &'a str) -> Option<&'a str> {
    parse_match_line(default_path(paths), line)
        .map(|(path, _, _)| path)
        .or_else(|| parse_context_line_path(default_path(paths), line))
}

fn default_path(paths: &[String]) -> &str {
    match paths.first().map(String::as_str) {
        Some("-") => "<stdin>",
        Some(path) => path,
        None => ".",
    }
}

fn parse_context_line_path<'a>(default_path: &'a str, line: &'a str) -> Option<&'a str> {
    if let Some((line_number, _)) = line.split_once(':') {
        if line_number.parse::<usize>().is_ok() {
            return Some(default_path);
        }
    }
    if line
        .split_once('-')
        .is_some_and(|(line_number, _)| line_number.parse::<usize>().is_ok())
    {
        return Some(default_path);
    }
    for (separator, _) in line.match_indices('-') {
        let path = &line[..separator];
        if path.is_empty() {
            continue;
        }
        let rest = &line[separator + 1..];
        if rest
            .split_once('-')
            .is_some_and(|(line_number, _)| line_number.parse::<usize>().is_ok())
        {
            return Some(path);
        }
    }
    None
}

pub(super) fn format_file_list(stdout: &str, max_results: Option<usize>) -> String {
    let file_count = count_non_empty_lines(stdout);
    if file_count == 0 {
        return "0 files".to_string();
    }

    let shown_count = max_results.unwrap_or(file_count).min(file_count);
    let mut result = if shown_count == file_count {
        format!("{file_count} files\n")
    } else {
        format!("{shown_count} shown of {file_count} files\n")
    };

    for file in non_empty_lines(stdout).take(shown_count) {
        result.push_str(file);
        result.push('\n');
    }
    if shown_count < file_count {
        result.push_str(&format!(
            "... +{} more files hidden by --max-results\n",
            file_count - shown_count
        ));
    }
    result.trim_end().to_string()
}

pub(super) fn format_files_with_matches(
    pattern: &str,
    stdout: &str,
    max_results: Option<usize>,
) -> String {
    let file_count = count_non_empty_lines(stdout);
    if file_count == 0 {
        return format!("0 matches for '{pattern}'");
    }

    let shown_count = max_results.unwrap_or(file_count).min(file_count);
    let mut result = if shown_count == file_count {
        format!("{file_count} files with matches for '{pattern}'\n")
    } else {
        format!("{shown_count} shown of {file_count} files with matches for '{pattern}'\n")
    };
    for file in non_empty_lines(stdout).take(shown_count) {
        result.push_str(file);
        result.push('\n');
    }
    if shown_count < file_count {
        result.push_str(&format!(
            "... +{} more files hidden by --max-results\n",
            file_count - shown_count
        ));
    }
    result.trim_end().to_string()
}

fn count_non_empty_lines(stdout: &str) -> usize {
    non_empty_lines(stdout).count()
}

fn non_empty_lines(stdout: &str) -> impl Iterator<Item = &str> {
    stdout.lines().filter(|line| !line.trim().is_empty())
}

pub(super) fn no_matches_outcome(
    pattern: &str,
    stderr: &str,
    exit_code: i32,
    hint: Option<&str>,
) -> ProxyOutcome {
    let stdout = if exit_code == 2 {
        String::new()
    } else if let Some(hint) = hint {
        format!("0 matches for '{pattern}'\n{hint}")
    } else {
        format!("0 matches for '{pattern}'")
    };
    ProxyOutcome {
        stdout,
        stderr: stderr.trim_end().to_string(),
        exit_code,
        observation: None,
    }
}

pub(super) fn basic_alternation_hint(
    patterns: &[String],
    options: &GrepOptions,
) -> Option<&'static str> {
    if options.extended_regexp || options.fixed_strings {
        return None;
    }
    if patterns
        .iter()
        .any(|pattern| contains_unescaped_pipe(pattern))
    {
        Some("hint: `cx grep` uses basic grep-style patterns, so bare `|` is literal; use `cx grep -E` or `cx rg` for alternation.")
    } else {
        None
    }
}

fn contains_unescaped_pipe(value: &str) -> bool {
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '|' {
            return true;
        }
    }
    false
}

pub(super) fn display_patterns(patterns: &[String]) -> String {
    match patterns {
        [] => String::new(),
        [pattern] => pattern.clone(),
        _ => patterns.join(" | "),
    }
}

pub(super) fn format_matches(
    pattern: &str,
    paths: &[String],
    stdout: &str,
    options: &GrepOptions,
) -> Option<String> {
    let matches = accumulate_matches(paths, stdout, options)?;
    if matches.total == 0 {
        return Some(format!("0 matches for '{pattern}'"));
    }

    let mut result = String::new();
    if matches.shown < matches.total {
        result.push_str(&format!(
            "{} shown of {} matches for '{pattern}' in {} files\n",
            matches.shown,
            matches.total,
            matches.files.len()
        ));
    } else {
        result.push_str(&format!(
            "{} matches for '{pattern}' in {} files\n",
            matches.total,
            matches.files.len()
        ));
    }

    let per_file_cap = if options.max_results.is_some() {
        usize::MAX
    } else {
        8
    };
    for (file, file_matches) in matches.files.iter() {
        let file_count = if options.max_results.is_some() {
            file_matches.shown.len()
        } else {
            file_matches.total
        };
        result.push_str(&format!("\n[file] {file} ({file_count})\n"));
        for (line_number, content) in file_matches.shown.iter().take(per_file_cap) {
            result.push_str(&format!(
                "  {:>4}: {}\n",
                line_number,
                truncate_match_line(content)
            ));
        }
        if file_count > per_file_cap {
            result.push_str(&format!(
                "  ... +{} more in file\n",
                file_count - per_file_cap
            ));
        }
    }

    if matches.shown < matches.total {
        result.push_str(&format!(
            "\n... +{} more matches hidden by --max-results\n",
            matches.total - matches.shown
        ));
    }

    Some(result.trim_end().to_string())
}

fn truncate_match_line(content: &str) -> String {
    truncate_generated_line(content, MATCH_LINE_PREVIEW_CHARS)
}

pub(super) fn truncate_output_lines(output: &str) -> String {
    let mut result = String::new();
    let mut first = true;
    for line in output.lines() {
        if first {
            first = false;
        } else {
            result.push('\n');
        }
        result.push_str(&truncate_match_line(line));
    }
    result
}

#[derive(Debug, Default)]
struct MatchAccumulator {
    total: usize,
    shown: usize,
    files: BTreeMap<String, FileMatches>,
}

#[derive(Debug, Default)]
struct FileMatches {
    total: usize,
    shown: Vec<(usize, String)>,
}

fn accumulate_matches(
    paths: &[String],
    stdout: &str,
    options: &GrepOptions,
) -> Option<MatchAccumulator> {
    let default_path = default_path(paths);
    let mut matches = MatchAccumulator::default();

    for line in stdout.lines() {
        let (file, line_number, content) = parse_match_line(default_path, line)?;
        matches.total += 1;

        if let Some(max_results) = options.max_results {
            if matches.shown >= max_results {
                continue;
            }
            matches
                .files
                .entry(file.to_string())
                .or_default()
                .shown
                .push((line_number, content.trim().to_string()));
            matches.shown += 1;
            continue;
        }

        let file_matches = matches.files.entry(file.to_string()).or_default();
        file_matches.total += 1;
        if file_matches.shown.len() < 8 {
            file_matches
                .shown
                .push((line_number, content.trim().to_string()));
        }
        matches.shown += 1;
    }
    Some(matches)
}

fn parse_match_line<'a>(default_path: &'a str, line: &'a str) -> Option<(&'a str, usize, &'a str)> {
    let (first, rest) = line.split_once(':')?;
    if let Ok(line_number) = first.parse::<usize>() {
        return Some((default_path, line_number, rest));
    }

    let (line_number, content) = rest.split_once(':')?;
    Some((first, line_number.parse::<usize>().unwrap_or(0), content))
}
