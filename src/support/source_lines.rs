use crate::support::utils::truncate;

pub(crate) fn truncate_generated_line(value: &str, max_chars: usize) -> String {
    if should_preserve_long_source_line(value, max_chars) {
        return value.to_string();
    }
    truncate_with_marker(value, max_chars)
}

pub(crate) fn truncate_human_source_range_line(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars || !contains_generated_blob_payload(value, max_chars) {
        return value.to_string();
    }
    truncate_with_marker(value, max_chars)
}

fn should_preserve_long_source_line(line: &str, preview_chars: usize) -> bool {
    if line.chars().count() <= preview_chars {
        return false;
    }
    let Some(source) = source_text_candidate(line) else {
        return false;
    };
    !contains_generated_blob_token(source, preview_chars)
}

fn source_text_candidate(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if looks_like_source_text(trimmed) {
        return Some(trimmed);
    }
    search_result_content(trimmed).filter(|content| looks_like_source_text(content))
}

fn search_result_content(value: &str) -> Option<&str> {
    for (index, _) in value.match_indices(':') {
        let prefix = &value[..index];
        let line_number = prefix.rsplit_once(':').map_or(prefix, |(_, tail)| tail);
        if !line_number.is_empty() && line_number.chars().all(|ch| ch.is_ascii_digit()) {
            return value.get(index + 1..).map(str::trim_start);
        }
    }
    None
}

fn looks_like_source_text(trimmed: &str) -> bool {
    looks_like_source_definition(trimmed)
        || looks_like_source_statement(trimmed)
        || looks_like_embedded_source_literal(trimmed)
        || looks_like_jsx_source_line(trimmed)
        || looks_like_css_declaration(trimmed)
}

fn looks_like_embedded_source_literal(trimmed: &str) -> bool {
    starts_with_string_literal(trimmed)
        && ((trimmed.match_indices("\\n").nth(1).is_some() && has_code_separator_density(trimmed))
            || looks_like_structured_source_literal(trimmed))
}

fn starts_with_string_literal(value: &str) -> bool {
    if matches!(value.chars().next(), Some('"' | '\'' | '`')) || value.starts_with("b\"") {
        return true;
    }
    let raw = value.strip_prefix("br").or_else(|| value.strip_prefix('r'));
    raw.is_some_and(|rest| rest.trim_start_matches('#').starts_with('"'))
}

fn looks_like_structured_source_literal(value: &str) -> bool {
    let structural_chars = value
        .chars()
        .filter(|ch| {
            matches!(
                ch,
                ',' | ':' | ';' | '=' | '(' | ')' | '{' | '}' | '[' | ']'
            )
        })
        .count();
    let format_placeholders = value.matches('{').count().min(value.matches('}').count());
    let json_fields = value.matches("\":").count();
    structural_chars >= 12 && (format_placeholders >= 4 || json_fields >= 2)
}

fn looks_like_source_definition(trimmed: &str) -> bool {
    definition_prefix(trimmed)
        || (trimmed.contains("=>") && has_code_separator_density(trimmed))
        || (trimmed.contains(" = {") && has_code_separator_density(trimmed))
}

fn definition_prefix(trimmed: &str) -> bool {
    matches!(
        trimmed
            .split(|ch: char| ch.is_whitespace() || ch == '(' || ch == '<' || ch == '{')
            .next(),
        Some(
            "function"
                | "class"
                | "interface"
                | "type"
                | "enum"
                | "struct"
                | "impl"
                | "trait"
                | "def"
                | "fn"
                | "pub"
                | "const"
                | "let"
                | "var"
                | "export"
                | "async"
        )
    )
}

fn looks_like_source_statement(trimmed: &str) -> bool {
    source_statement_prefix(trimmed)
        && (has_code_separator_density(trimmed) || looks_like_regex_literal(trimmed))
}

fn source_statement_prefix(trimmed: &str) -> bool {
    matches!(
        first_source_token(trimmed),
        Some(
            "assert"
                | "expect"
                | "await"
                | "return"
                | "if"
                | "for"
                | "while"
                | "throw"
                | "console"
                | "page"
                | "frame"
                | "test"
                | "it"
                | "describe"
        )
    )
}

fn first_source_token(trimmed: &str) -> Option<&str> {
    trimmed
        .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '(' | '{' | '['))
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | '<' | '{' | '.' | '['))
        .next()
        .filter(|token| !token.is_empty())
}

fn has_code_separator_density(value: &str) -> bool {
    let separators = value
        .chars()
        .filter(|ch| matches!(ch, ',' | ':' | '=' | '(' | ')' | '{' | '}' | '[' | ']'))
        .count();
    separators >= 8 && value.split_whitespace().count() >= 8
}

fn looks_like_regex_literal(value: &str) -> bool {
    let Some(start) = value.find('/') else {
        return false;
    };
    let rest = &value[start + 1..];
    let Some(end) = rest.rfind('/') else {
        return false;
    };
    let body = &rest[..end];
    body.len() >= 3
        && body.chars().any(|ch| {
            matches!(
                ch,
                '|' | '[' | ']' | '(' | ')' | '?' | '+' | '*' | '{' | '}' | '\\' | '^' | '$'
            )
        })
}

fn looks_like_jsx_source_line(trimmed: &str) -> bool {
    let Some(tag_start) = trimmed.find('<') else {
        return false;
    };
    let after_tag = &trimmed[tag_start + 1..];
    let Some(first) = after_tag.chars().next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    let jsx = &trimmed[tag_start..];
    jsx.contains('>') && jsx_attribute_count(jsx) >= 2
}

fn jsx_attribute_count(value: &str) -> usize {
    value
        .split_whitespace()
        .filter(|segment| {
            let Some((name, rhs)) = segment.split_once('=') else {
                return false;
            };
            is_jsx_attribute_name(name) && !rhs.trim().is_empty()
        })
        .count()
}

fn is_jsx_attribute_name(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == ':')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
}

fn looks_like_css_declaration(trimmed: &str) -> bool {
    trimmed.contains('{')
        && trimmed.contains('}')
        && trimmed.contains(':')
        && trimmed.contains(';')
        && css_property_count(trimmed) > 0
}

fn css_property_count(value: &str) -> usize {
    value
        .split(';')
        .filter_map(css_property_name)
        .filter(|name| is_css_property_name(name))
        .count()
}

fn css_property_name(segment: &str) -> Option<&str> {
    let declaration = segment
        .rsplit_once(['{', '}'])
        .map_or(segment, |(_, after)| after)
        .trim();
    let (name, value) = declaration.split_once(':')?;
    (!value.trim().is_empty()).then_some(name.trim())
}

fn is_css_property_name(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '-')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn contains_generated_blob_token(line: &str, preview_chars: usize) -> bool {
    max_blob_token_len(line) >= preview_chars / 2
}

fn contains_generated_blob_payload(line: &str, preview_chars: usize) -> bool {
    contains_generated_blob_token(line, preview_chars)
        || accumulated_blob_run_chars(line, (preview_chars / 4).max(32)) >= preview_chars
}

fn accumulated_blob_run_chars(line: &str, minimum_run: usize) -> usize {
    let mut total = 0usize;
    let mut current = 0usize;
    for ch in line.chars().chain(std::iter::once(' ')) {
        if is_blob_token_char(ch) {
            current += 1;
        } else {
            if current >= minimum_run {
                total += current;
            }
            current = 0;
        }
    }
    total
}

fn max_blob_token_len(line: &str) -> usize {
    let mut max_len = 0usize;
    let mut current = 0usize;
    for ch in line.chars() {
        if is_blob_token_char(ch) {
            current += 1;
            max_len = max_len.max(current);
        } else {
            current = 0;
        }
    }
    max_len
}

fn is_blob_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '_' | '-' | '=')
}

fn truncate_with_marker(value: &str, max_chars: usize) -> String {
    let marker = " ... [truncated]";
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= marker.chars().count() + 3 {
        return truncate(value, max_chars);
    }

    let prefix_len = max_chars - marker.chars().count();
    let prefix: String = value.chars().take(prefix_len).collect();
    format!("{prefix}{marker}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_location_prefix_preserves_structured_source() {
        let line = format!(
            "src/archive.rs:330:r#\"{{\"rawBytes\":{{raw_bytes}},\"rawChars\":{{raw_chars}},\"rawLines\":{{raw_lines}},\"rawTokens\":{{raw_tokens}},\"emittedBytes\":{{emitted_bytes}},\"emittedChars\":{{emitted_chars}}}}\"#{}",
            ",".repeat(100)
        );
        assert_eq!(truncate_generated_line(&line, 240), line);
    }

    #[test]
    fn long_search_path_does_not_trigger_the_source_blob_guard() {
        let path = format!("src/{}/archive.rs", "deep-segment/".repeat(16));
        let line = format!(
            "{path}:330:r#\"{{\"rawBytes\":{{raw_bytes}},\"rawChars\":{{raw_chars}},\"rawLines\":{{raw_lines}},\"rawTokens\":{{raw_tokens}},\"emittedBytes\":{{emitted_bytes}},\"emittedChars\":{{emitted_chars}}}}\"#{}",
            ",".repeat(100)
        );
        assert_eq!(truncate_generated_line(&line, 240), line);
    }

    #[test]
    fn generated_token_wins_over_structured_wrapper() {
        let blob = "A".repeat(400);
        let line = format!("r#\"{{\"kind\":\"fixture\",\"payload\":\"{blob}\",\"ok\":true}}\"#;");
        assert!(truncate_generated_line(&line, 240).contains("[truncated]"));
    }

    #[test]
    fn human_source_ranges_preserve_unclassified_code_but_not_blob_tokens() {
        let source = format!(
            "{{\"description\", \"{}\"}}}},",
            "This is a long C++ command registry description with ordinary words and punctuation. "
                .repeat(5)
        );
        assert_eq!(truncate_human_source_range_line(&source, 240), source);

        let blob = format!("const payload = \"{}\";", "A".repeat(400));
        assert!(truncate_human_source_range_line(&blob, 240).contains("[truncated]"));
    }
}
