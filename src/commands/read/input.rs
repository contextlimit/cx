use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use anyhow::{Context, Result};

use crate::support::document_formats::is_human_source_path;
use crate::support::runner::ProxyOutcome;

use super::window::{
    apply_line_range, bound_human_source_range_lines_with_status, bound_output_lines_with_status,
    format_with_line_numbers_from,
};
use super::{read_source_label, ReadOptions, ReadRange};

pub(super) fn run_line_range(
    path: &Path,
    options: &ReadOptions,
    line_range: ReadRange,
) -> Result<ProxyOutcome> {
    let mut rendered = read_line_range(path, line_range)?;
    let raw_range = rendered.clone();
    let mut transformed = false;
    if !options.raw {
        let bounded = if is_human_source_path(path) {
            bound_human_source_range_lines_with_status(&rendered)
        } else {
            bound_output_lines_with_status(&rendered)
        };
        rendered = bounded.text;
        transformed = bounded.changed;
    }
    if options.line_numbers {
        rendered = format_with_line_numbers_from(&rendered, line_range.display_start());
        transformed = true;
    }
    let mut outcome = ProxyOutcome::success(rendered)
        .with_raw_output(read_source_label(path), &raw_range)
        .with_expansion_reason("read-formatting");
    if !transformed {
        outcome.observation = outcome
            .observation
            .take()
            .map(|observation| observation.with_preserved_stream_termination());
    }
    Ok(outcome)
}

pub(super) fn read_content(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read stdin")?;
        return Ok(buffer);
    }
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

fn read_line_range(path: &Path, line_range: ReadRange) -> Result<String> {
    if path == Path::new("-") {
        let content = read_content(path)?;
        return Ok(apply_line_range(&content, line_range));
    }

    let start = line_range.display_start();
    if let Some(end) = line_range.end {
        if end < start {
            return Ok(String::new());
        }
    }

    let file =
        fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let first_line = start.max(1);
    let mut output = String::new();
    let mut line = String::new();
    let mut line_number = 0usize;

    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if bytes == 0 {
            break;
        }
        line_number += 1;
        if line_number < first_line {
            continue;
        }
        if line_range.end.is_some_and(|end| line_number > end) {
            break;
        }
        output.push_str(&line);
        if line_range.end.is_some_and(|end| line_number == end) {
            break;
        }
    }

    Ok(output)
}
