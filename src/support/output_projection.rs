use serde::de::IgnoredAny;
use serde::Deserialize;

pub(crate) const GENERATED_LINE_PREVIEW_CHARS: usize = 1_200;

pub(crate) fn bound_generated_lines(output: &str, max_chars: usize) -> String {
    if output.is_empty() || max_chars == 0 {
        return output.to_string();
    }

    let mut projected = String::with_capacity(output.len().min(max_chars.saturating_mul(2)));
    for segment in output.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map_or((segment, ""), |line| (line, "\n"));
        if line.chars().count() > max_chars && looks_like_generated_output(line, max_chars) {
            projected.push_str(&truncate_middle(line, max_chars));
        } else {
            projected.push_str(line);
        }
        projected.push_str(newline);
    }
    projected
}

fn looks_like_generated_output(line: &str, max_chars: usize) -> bool {
    let trimmed = line.trim();
    looks_like_json(trimmed)
        || looks_like_serialized_markup(trimmed)
        || max_blob_token_len(trimmed) >= max_chars / 2
}

fn looks_like_json(value: &str) -> bool {
    let candidate = value.strip_prefix("data:").map_or(value, str::trim_start);
    if !matches!(
        candidate.as_bytes().first().copied(),
        Some(b'{' | b'[' | b'"')
    ) {
        return false;
    }

    let mut deserializer = serde_json::Deserializer::from_str(candidate);
    IgnoredAny::deserialize(&mut deserializer).is_ok() && deserializer.end().is_ok()
}

fn looks_like_serialized_markup(value: &str) -> bool {
    let prefix = value
        .chars()
        .take(32)
        .collect::<String>()
        .to_ascii_lowercase();
    prefix.starts_with("<!doctype html")
        || prefix.starts_with("<html")
        || prefix.starts_with("<?xml")
}

fn max_blob_token_len(value: &str) -> usize {
    let mut max_len = 0usize;
    let mut current = 0usize;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '_' | '-' | '=') {
            current += 1;
            max_len = max_len.max(current);
        } else {
            current = 0;
        }
    }
    max_len
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let total_chars = value.chars().count();
    let marker = format!(" ... [generated line truncated; {total_chars} chars total] ... ");
    let marker_chars = marker.chars().count();
    if max_chars <= marker_chars + 2 {
        return value.chars().take(max_chars).collect();
    }

    let visible_chars = max_chars - marker_chars;
    let head_chars = visible_chars * 2 / 3;
    let tail_chars = visible_chars - head_chars;
    let head = value.chars().take(head_chars).collect::<String>();
    let mut tail = value.chars().rev().take(tail_chars).collect::<Vec<_>>();
    tail.reverse();
    format!("{head}{marker}{}", tail.into_iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_json_and_preserves_both_ends() {
        let raw = format!(
            r#"{{"status":"ok","payload":"{}","tail":"complete"}}"#,
            "A".repeat(2_000)
        );
        let projected = bound_generated_lines(&raw, 240);

        assert!(projected.chars().count() <= 240);
        assert!(projected.starts_with(r#"{"status":"ok""#));
        assert!(projected.ends_with(r#""tail":"complete"}"#));
        assert!(projected.contains("generated line truncated"));
        assert!(projected.contains(&format!("{} chars total", raw.chars().count())));
    }

    #[test]
    fn bounds_sse_json_and_serialized_markup() {
        let sse = format!(r#"data: {{"payload":"{}","done":true}}"#, "B".repeat(2_000));
        let html = format!(
            "<!doctype html><html><body>{}</body></html>",
            "x".repeat(2_000)
        );

        assert!(bound_generated_lines(&sse, 240).contains("generated line truncated"));
        assert!(bound_generated_lines(&html, 240).contains("generated line truncated"));
    }

    #[test]
    fn bounds_blob_tokens_but_preserves_source_css_and_prose() {
        let blob = format!("payload={}", "A".repeat(2_000));
        let source = format!(
            "const matcher = /{}/; return matcher.test(candidate);",
            "route|path|dashboard|projects|migration|brain|agents|skills|".repeat(40)
        );
        let css = format!(
            ".panel{{--panel-copy:'{}';color:var(--panel-text,#fff);display:grid;}}",
            "human readable css value ".repeat(90)
        );
        let prose = "This is deliberately long human-authored prose. ".repeat(80);

        assert!(bound_generated_lines(&blob, 240).contains("generated line truncated"));
        assert_eq!(bound_generated_lines(&source, 240), source);
        assert_eq!(bound_generated_lines(&css, 240), css);
        assert_eq!(bound_generated_lines(&prose, 240), prose);
    }

    #[test]
    fn preserves_line_structure_and_trailing_newline() {
        let raw = format!("before\n{{\"payload\":\"{}\"}}\nafter\n", "Z".repeat(2_000));
        let projected = bound_generated_lines(&raw, 240);

        assert!(projected.starts_with("before\n"));
        assert!(projected.ends_with("\nafter\n"));
        assert_eq!(projected.lines().count(), 3);
    }
}
