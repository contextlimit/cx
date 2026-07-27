use std::collections::VecDeque;
use std::fmt::Write as _;

use crate::support::{
    source_lines::{truncate_generated_line, truncate_human_source_range_line},
    utils::fallback_window,
};

use super::{ReadRange, READ_LINE_PREVIEW_CHARS};

pub(super) fn apply_window(
    content: &str,
    head_lines: Option<usize>,
    tail_lines: Option<usize>,
) -> String {
    if let Some(lines) = tail_lines {
        if lines == 0 {
            return String::new();
        }
        let mut tail = VecDeque::with_capacity(lines);
        for line in content.lines() {
            if tail.len() == lines {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        return join_lines(tail);
    }

    if let Some(lines) = head_lines {
        if lines == 0 {
            return String::new();
        }
        return join_lines(content.lines().take(lines));
    }

    content.to_string()
}

pub(super) fn format_with_line_numbers(content: &str) -> String {
    format_with_line_numbers_from(content, 1)
}

pub(super) fn format_with_line_numbers_from(content: &str, start_at: usize) -> String {
    let line_count = content.lines().count();
    if line_count == 0 {
        return String::new();
    }
    let end = start_at.saturating_add(line_count.saturating_sub(1));
    let width = end.max(1).to_string().len();
    let mut output = String::new();
    for (index, line) in content.lines().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        write!(
            &mut output,
            "{:>width$} │ {}",
            start_at + index,
            line,
            width = width
        )
        .expect("format line numbers into string");
    }
    output
}

pub(super) fn apply_line_range(content: &str, line_range: ReadRange) -> String {
    let start = line_range.display_start();
    if line_range.end.is_some_and(|end| end < start) {
        return String::new();
    }

    let start_index = start.saturating_sub(1);
    content
        .split_inclusive('\n')
        .enumerate()
        .skip(start_index)
        .take_while(|(index, _)| line_range.end.is_none_or(|end| *index < end))
        .map(|(_, line)| line)
        .collect()
}

pub(super) fn bound_output_lines(content: &str) -> String {
    bound_output_lines_with_status(content).text
}

pub(super) struct BoundedOutput {
    pub(super) text: String,
    pub(super) changed: bool,
}

pub(super) fn bound_output_lines_with_status(content: &str) -> BoundedOutput {
    bound_output_lines_with(content, truncate_generated_line)
}

pub(super) fn bound_human_source_range_lines_with_status(content: &str) -> BoundedOutput {
    bound_output_lines_with(content, truncate_human_source_range_line)
}

fn bound_output_lines_with(
    content: &str,
    truncate_line: fn(&str, usize) -> String,
) -> BoundedOutput {
    let mut output = String::new();
    let mut changed = false;
    for line in content.split_inclusive('\n') {
        let (body, ending) = split_line_ending(line);
        let bounded = truncate_line(body, READ_LINE_PREVIEW_CHARS);
        changed |= bounded != body;
        output.push_str(&bounded);
        output.push_str(ending);
    }
    BoundedOutput {
        text: output,
        changed,
    }
}

pub(super) fn cap_lines(content: &str, max_lines: usize) -> String {
    if content.lines().nth(max_lines).is_none() {
        return content.to_string();
    }
    let head = (max_lines * 2 / 3).max(1);
    let tail = max_lines.saturating_sub(head).max(1);
    fallback_window(content, head, tail)
}

fn join_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut output = String::new();
    for (index, line) in lines.into_iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(line);
    }
    output
}

fn split_line_ending(line: &str) -> (&str, &str) {
    let Some(without_lf) = line.strip_suffix('\n') else {
        return (line, "");
    };
    without_lf
        .strip_suffix('\r')
        .map_or((without_lf, "\n"), |body| (body, "\r\n"))
}
