use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::support::utils::truncate;

pub fn summarize_logs(content: &str) -> String {
    let mut errors: HashMap<String, usize> = HashMap::new();
    let mut warnings: HashMap<String, usize> = HashMap::new();
    let mut infos = 0usize;
    let mut original_errors: Vec<String> = Vec::new();
    let mut original_warnings: Vec<String> = Vec::new();

    for line in content.lines() {
        let normalized = normalize_log_line(line);
        let lower = line.to_ascii_lowercase();
        if lower.contains("error") || lower.contains("fatal") || lower.contains("panic") {
            let count = errors.entry(normalized).or_insert(0);
            if *count == 0 {
                original_errors.push(line.to_string());
            }
            *count += 1;
        } else if lower.contains("warn") {
            let count = warnings.entry(normalized).or_insert(0);
            if *count == 0 {
                original_warnings.push(line.to_string());
            }
            *count += 1;
        } else if lower.contains("info") {
            infos += 1;
        }
    }

    let mut result = Vec::new();
    result.push("Log Summary".to_string());
    result.push(format!(
        "  [error] {} errors ({} unique)",
        errors.values().sum::<usize>(),
        errors.len()
    ));
    result.push(format!(
        "  [warn] {} warnings ({} unique)",
        warnings.values().sum::<usize>(),
        warnings.len()
    ));
    result.push(format!("  [info] {} info messages", infos));

    if !errors.is_empty() {
        result.push(String::new());
        result.push("[ERRORS]".to_string());
        push_group_lines(&mut result, &errors, &original_errors, 10);
    }

    if !warnings.is_empty() {
        result.push(String::new());
        result.push("[WARNINGS]".to_string());
        push_group_lines(&mut result, &warnings, &original_warnings, 5);
    }

    result.join("\n")
}

fn push_group_lines(
    result: &mut Vec<String>,
    counts: &HashMap<String, usize>,
    originals: &[String],
    limit: usize,
) {
    let mut entries: Vec<_> = counts.iter().collect();
    entries.sort_by(|left, right| right.1.cmp(left.1));

    for (index, (normalized, count)) in entries.iter().take(limit).enumerate() {
        let original = originals
            .iter()
            .find(|line| normalize_log_line(line) == **normalized)
            .cloned()
            .unwrap_or_else(|| (*normalized).to_string());
        let line = truncate(&original, 100);
        if **count > 1 {
            result.push(format!("  [x{}] {}", count, line));
        } else {
            result.push(format!("  {}", line));
        }
        if index + 1 == limit && entries.len() > limit {
            result.push(format!("  ... +{} more", entries.len() - limit));
        }
    }
}

fn normalize_log_line(line: &str) -> String {
    let mut normalized = timestamp_re().replace(line, "").to_string();
    normalized = uuid_re().replace_all(&normalized, "<UUID>").to_string();
    normalized = hex_re().replace_all(&normalized, "<HEX>").to_string();
    normalized = number_re().replace_all(&normalized, "<NUM>").to_string();
    normalized = path_re().replace_all(&normalized, "<PATH>").to_string();
    normalized.trim().to_string()
}

fn timestamp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}[.,]?\d*\s*")
            .expect("timestamp regex")
    })
}

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
            .expect("uuid regex")
    })
}

fn hex_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"0x[0-9a-fA-F]+").expect("hex regex"))
}

fn number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{4,}\b").expect("number regex"))
}

fn path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/[\w./\-]+").expect("path regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_logs_deduplicates_repeated_errors() {
        let logs = "2024-01-01 10:00:00 ERROR: Connection failed to /tmp/x\n\
                    2024-01-01 10:00:01 ERROR: Connection failed to /tmp/y\n\
                    2024-01-01 10:00:02 WARN: Retrying connection\n\
                    2024-01-01 10:00:03 INFO: Connected";
        let result = summarize_logs(logs);
        assert!(result.contains("[x2]"));
        assert!(result.contains("[WARNINGS]"));
    }
}
