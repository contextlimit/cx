use std::env;
use std::path::Path;

use super::language::{is_attribute_line, is_doc_comment, should_keep_signature, Language};
use super::window::cap_lines;
use super::{
    ReadMode, ReadOptions, DEFAULT_AGGRESSIVE_MAX_LINES, DEFAULT_AUTO_THRESHOLD_BYTES,
    DEFAULT_AUTO_THRESHOLD_LINES,
};

pub(super) fn effective_mode(content: &str, options: &ReadOptions) -> ReadMode {
    if options.raw {
        return ReadMode::Normal;
    }
    match options.mode {
        ReadMode::Normal
            if options.auto_aggressive
                && options.head_lines.is_none()
                && options.tail_lines.is_none()
                && options.line_range.is_none()
                && should_auto_aggressive(content) =>
        {
            ReadMode::Aggressive
        }
        other => other,
    }
}

pub(super) fn should_auto_aggressive(content: &str) -> bool {
    let line_threshold = env_usize("CX_READ_AUTO_THRESHOLD_LINES", DEFAULT_AUTO_THRESHOLD_LINES);
    let byte_threshold = env_usize("CX_READ_AUTO_THRESHOLD_BYTES", DEFAULT_AUTO_THRESHOLD_BYTES);
    content.len() > byte_threshold || content.lines().count() > line_threshold
}

fn aggressive_line_cap(explicit: Option<usize>) -> usize {
    explicit.unwrap_or_else(|| env_usize("CX_READ_MAX_LINES", DEFAULT_AGGRESSIVE_MAX_LINES))
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

pub(super) fn render_aggressive(
    content: &str,
    _path: &Path,
    language: Language,
    explicit_max_lines: Option<usize>,
) -> String {
    let max_lines = aggressive_line_cap(explicit_max_lines).max(1);
    if matches!(language, Language::Data | Language::Unknown) {
        return cap_lines(&collapse_blank_lines(content), max_lines);
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut kept = Vec::new();
    let mut pending_docs = Vec::new();
    let mut pending_attrs = Vec::new();
    let mut skip_depth = 0i32;
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if skip_depth > 0 {
            skip_depth += brace_delta(trimmed);
            if skip_depth <= 0 {
                skip_depth = 0;
            }
            index += 1;
            continue;
        }

        if trimmed.is_empty() {
            index += 1;
            continue;
        }

        if is_doc_comment(language, trimmed) {
            pending_docs.push(line.trim_end().to_string());
            index += 1;
            continue;
        }

        if is_attribute_line(language, trimmed) {
            pending_attrs.push(line.trim_end().to_string());
            index += 1;
            continue;
        }

        if should_keep_signature(language, trimmed) {
            flush_pending_lines(&mut kept, &mut pending_docs, &mut pending_attrs);
            let (signature_lines, next_index, opens_block) =
                collect_signature_block(&lines, index, language);
            kept.extend(signature_lines);
            if opens_block {
                let mut depth = 0i32;
                for line in &lines[index..next_index] {
                    depth += brace_delta(line.trim());
                }
                if depth > 0 {
                    skip_depth = depth;
                }
            }
            index = next_index;
            continue;
        }

        pending_docs.clear();
        pending_attrs.clear();
        index += 1;
    }

    if kept.is_empty() {
        return cap_lines(&collapse_blank_lines(content), max_lines);
    }

    let rendered = collapse_blank_lines(&kept.join("\n"));
    cap_lines(&rendered, max_lines)
}

pub(super) fn filter_content(content: &str, language: Language) -> String {
    if matches!(language, Language::Data) {
        return collapse_blank_lines(content);
    }

    let mut result = String::new();
    let mut in_block_comment = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }

        if matches!(
            language,
            Language::Rust | Language::JavaScript | Language::TypeScript | Language::Go
        ) {
            if trimmed.starts_with("/*") {
                if !trimmed.contains("*/") {
                    in_block_comment = true;
                }
                continue;
            }
            if trimmed.starts_with("//")
                && !trimmed.starts_with("///")
                && !trimmed.starts_with("//!")
            {
                continue;
            }
        }

        if matches!(language, Language::Python | Language::Shell)
            && trimmed.starts_with('#')
            && !trimmed.starts_with("#!")
        {
            continue;
        }

        result.push_str(line.trim_end());
        result.push('\n');
    }

    collapse_blank_lines(&result)
}

pub(super) fn collapse_blank_lines(content: &str) -> String {
    let mut result = String::new();
    let mut blank_run = 0usize;

    for line in content.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 2 {
                result.push('\n');
            }
            continue;
        }
        blank_run = 0;
        result.push_str(line.trim_end());
        result.push('\n');
    }

    result.trim_end_matches('\n').to_string()
}

fn flush_pending_lines(target: &mut Vec<String>, docs: &mut Vec<String>, attrs: &mut Vec<String>) {
    target.append(docs);
    target.append(attrs);
}

fn collect_signature_block(
    lines: &[&str],
    start: usize,
    language: Language,
) -> (Vec<String>, usize, bool) {
    let mut kept = vec![compact_inline_body(lines[start].trim_end())];
    let mut index = start + 1;
    let mut opens_block = line_opens_block(lines[start].trim());
    while index < lines.len() && kept.len() < 8 {
        let line = lines[index];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        let Some(previous) = kept.last() else {
            break;
        };
        let continues = continues_signature(previous);
        if should_keep_signature(language, trimmed) && !continues {
            break;
        }
        if !continues && !trimmed.starts_with("where") {
            break;
        }
        kept.push(compact_inline_body(line.trim_end()));
        opens_block = opens_block || line_opens_block(trimmed);
        index += 1;
        if signature_complete(trimmed) {
            break;
        }
    }
    (kept, index, opens_block)
}

fn continues_signature(previous: &str) -> bool {
    let trimmed = previous.trim_end();
    !signature_complete(trimmed)
}

fn signature_complete(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed.ends_with('{')
        || trimmed.ends_with(';')
        || trimmed.ends_with("}")
        || trimmed.ends_with("=>")
        || trimmed.ends_with("),")
}

fn line_opens_block(line: &str) -> bool {
    line.contains('{') && !line.contains("{}")
}

fn brace_delta(line: &str) -> i32 {
    let opens = line.chars().filter(|ch| *ch == '{').count() as i32;
    let closes = line.chars().filter(|ch| *ch == '}').count() as i32;
    opens - closes
}

fn compact_inline_body(line: &str) -> String {
    let Some(open_index) = line.find('{') else {
        return line.to_string();
    };
    let Some(close_index) = line.rfind('}') else {
        return line.to_string();
    };
    if close_index <= open_index + 1 {
        return line.to_string();
    }
    if line[open_index + 1..close_index].trim().is_empty() {
        return line.to_string();
    }
    format!(
        "{} ... {}",
        line[..=open_index].trim_end(),
        &line[close_index..]
    )
}
