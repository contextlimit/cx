use std::collections::BTreeSet;

use super::GrepOptions;

pub(super) fn normalize_ripgrep_patterns(
    patterns: &[String],
    options: &GrepOptions,
) -> Vec<String> {
    patterns
        .iter()
        .map(|pattern| normalize_ripgrep_pattern(pattern, options))
        .collect()
}

pub(super) fn normalize_ripgrep_pattern(pattern: &str, options: &GrepOptions) -> String {
    if options.fixed_strings || options.extended_regexp {
        return pattern.to_string();
    }

    let paired_bre_groups = paired_bre_group_markers(pattern);
    let mut normalized = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    let mut offset = 0usize;
    let mut in_char_class = false;
    let mut in_posix_char_class: Option<char> = None;

    while let Some(ch) = chars.next() {
        let current_offset = offset;
        offset += ch.len_utf8();
        if let Some(delimiter) = in_posix_char_class {
            normalized.push(ch);
            if ch == delimiter && chars.peek() == Some(&']') {
                let bracket = chars.next().unwrap();
                offset += bracket.len_utf8();
                normalized.push(bracket);
                in_posix_char_class = None;
            }
            continue;
        }

        if in_char_class {
            normalized.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    offset += next.len_utf8();
                    normalized.push(next);
                }
                continue;
            }
            if ch == '[' && matches!(chars.peek(), Some(':' | '.' | '=')) {
                let delimiter = chars.next().unwrap();
                offset += delimiter.len_utf8();
                normalized.push(delimiter);
                in_posix_char_class = Some(delimiter);
                continue;
            }
            if ch == ']' {
                in_char_class = false;
            }
            continue;
        }

        if ch == '\\' {
            match chars.next() {
                Some(next @ ('(' | ')')) => {
                    offset += next.len_utf8();
                    if paired_bre_groups.contains(&current_offset) {
                        normalized.push(next);
                    } else {
                        normalized.push('\\');
                        normalized.push(next);
                    }
                }
                Some(next @ ('{' | '}' | '+' | '?' | '|')) => {
                    offset += next.len_utf8();
                    normalized.push(next)
                }
                Some('<' | '>') => {
                    offset += 1;
                    normalized.push_str("\\b");
                }
                Some(next) => {
                    offset += next.len_utf8();
                    normalized.push('\\');
                    normalized.push(next);
                }
                None => normalized.push('\\'),
            }
            continue;
        }

        match ch {
            '[' => {
                in_char_class = true;
                normalized.push('[');
            }
            '(' | ')' | '{' | '}' | '+' | '?' | '|' => {
                normalized.push('\\');
                normalized.push(ch);
            }
            _ => {
                normalized.push(ch);
            }
        }
    }
    normalized
}

fn paired_bre_group_markers(pattern: &str) -> BTreeSet<usize> {
    let mut paired = BTreeSet::new();
    let mut stack = Vec::new();
    let mut chars = pattern.char_indices().peekable();
    let mut in_char_class = false;
    let mut in_posix_char_class: Option<char> = None;

    while let Some((index, ch)) = chars.next() {
        if let Some(delimiter) = in_posix_char_class {
            if ch == delimiter && matches!(chars.peek(), Some((_, ']'))) {
                chars.next();
                in_posix_char_class = None;
            }
            continue;
        }

        if in_char_class {
            if ch == '\\' {
                chars.next();
                continue;
            }
            if ch == '[' && matches!(chars.peek(), Some((_, ':' | '.' | '='))) {
                let (_, delimiter) = chars.next().unwrap();
                in_posix_char_class = Some(delimiter);
                continue;
            }
            if ch == ']' {
                in_char_class = false;
            }
            continue;
        }

        match ch {
            '[' => {
                in_char_class = true;
            }
            '\\' => {
                let Some((_, next)) = chars.next() else {
                    continue;
                };
                match next {
                    '(' => stack.push(index),
                    ')' => {
                        if let Some(open_index) = stack.pop() {
                            paired.insert(open_index);
                            paired.insert(index);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    paired
}
