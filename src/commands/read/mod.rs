mod input;
mod language;
mod render;
mod smart;
mod window;

#[cfg(test)]
mod tests;

use std::path::Path;

use anyhow::Result;

use crate::support::document_formats::is_exact_read_path;
use crate::support::runner::ProxyOutcome;
use crate::support::smart_read;

use input::{read_content, run_line_range};
use language::Language;
use render::{effective_mode, filter_content, render_aggressive};
use smart::render_local_smart_summary;
use window::{apply_window, bound_output_lines, format_with_line_numbers};

#[cfg(test)]
use render::should_auto_aggressive;
#[cfg(test)]
use window::apply_line_range;
#[cfg(test)]
use window::format_with_line_numbers_from;

pub const DEFAULT_AUTO_THRESHOLD_LINES: usize = 800;
pub const DEFAULT_AUTO_THRESHOLD_BYTES: usize = 64 * 1024;
pub const DEFAULT_AGGRESSIVE_MAX_LINES: usize = 120;
const READ_LINE_PREVIEW_CHARS: usize = 320;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadMode {
    #[default]
    Normal,
    Aggressive,
    Smart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOptions {
    pub head_lines: Option<usize>,
    pub tail_lines: Option<usize>,
    pub line_range: Option<ReadRange>,
    pub line_numbers: bool,
    pub raw: bool,
    pub mode: ReadMode,
    pub max_lines: Option<usize>,
    pub auto_aggressive: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            head_lines: None,
            tail_lines: None,
            line_range: None,
            line_numbers: false,
            raw: false,
            mode: ReadMode::Normal,
            max_lines: None,
            auto_aggressive: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRange {
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl ReadRange {
    fn display_start(self) -> usize {
        self.start.unwrap_or(1)
    }
}

pub fn run(path: &Path, options: &ReadOptions) -> Result<ProxyOutcome> {
    let exact_options;
    let options = if is_exact_read_path(path) {
        exact_options = exact_read_options(options);
        &exact_options
    } else {
        options
    };
    if let Some(line_range) = options.line_range {
        return run_line_range(path, options, line_range);
    }

    let content = read_content(path)?;
    let language = if path == Path::new("-") {
        Language::Unknown
    } else {
        Language::from_path(path)
    };

    let mut stderr = String::new();
    let mut rendered = if options.raw {
        content.clone()
    } else {
        match effective_mode(&content, options) {
            ReadMode::Normal => {
                let filtered = filter_content(&content, language);
                if filtered.trim().is_empty() && !content.trim().is_empty() {
                    content.clone()
                } else {
                    filtered
                }
            }
            ReadMode::Aggressive => render_aggressive(&content, path, language, options.max_lines),
            ReadMode::Smart => {
                let plugin_output = smart_read::summarize(
                    path,
                    &content,
                    language.as_smart_label(),
                    options.max_lines,
                );
                if let Some(note) = plugin_output.stderr_note {
                    stderr = note;
                }
                plugin_output
                    .summary
                    .unwrap_or_else(|| render_local_smart_summary(&content, path, language))
            }
        }
    };

    if !matches!(options.mode, ReadMode::Smart) {
        rendered = apply_window(&rendered, options.head_lines, options.tail_lines);
    }
    if !options.raw {
        rendered = bound_output_lines(&rendered);
    }
    if !matches!(options.mode, ReadMode::Smart) && options.line_numbers {
        rendered = format_with_line_numbers(&rendered);
    }

    Ok(ProxyOutcome {
        stdout: rendered,
        stderr,
        exit_code: 0,
        observation: None,
    }
    .with_raw_output(read_source_label(path), &content)
    .with_expansion_reason("read-formatting"))
}

fn exact_read_options(options: &ReadOptions) -> ReadOptions {
    let mut exact = options.clone();
    exact.raw = true;
    exact.mode = ReadMode::Normal;
    exact.max_lines = None;
    exact.auto_aggressive = false;
    exact
}

fn read_source_label(path: &Path) -> String {
    if path == Path::new("-") {
        "stdin".to_string()
    } else {
        path.display().to_string()
    }
}
