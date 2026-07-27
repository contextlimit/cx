use std::path::Path;

use crate::support::utils::truncate;

use super::language::{
    is_declaration_line, is_doc_comment, is_import_line, looks_like_test_line, Language,
};
use super::render::render_aggressive;
use super::DEFAULT_AGGRESSIVE_MAX_LINES;

pub(super) fn render_local_smart_summary(content: &str, path: &Path, language: Language) -> String {
    let aggressive = render_aggressive(
        content,
        path,
        language,
        Some(DEFAULT_AGGRESSIVE_MAX_LINES.min(80)),
    );
    let lines: Vec<&str> = aggressive.lines().collect();
    let signatures = lines
        .iter()
        .filter(|line| is_declaration_line(language, line.trim()))
        .take(4)
        .map(|line| truncate(line.trim(), 100))
        .collect::<Vec<_>>();
    let imports = lines
        .iter()
        .filter(|line| is_import_line(language, line.trim()))
        .take(3)
        .map(|line| truncate(line.trim(), 80))
        .collect::<Vec<_>>();
    let tests = lines
        .iter()
        .filter(|line| looks_like_test_line(language, line.trim()))
        .take(3)
        .map(|line| truncate(line.trim(), 80))
        .collect::<Vec<_>>();
    let doc_summary = first_doc_line(content, language);
    let path_label = if path == Path::new("-") {
        "stdin".to_string()
    } else {
        path.display().to_string()
    };
    let mut summary = vec![format!("Smart summary: {path_label}")];
    summary.push(format!(
        "Purpose: {}",
        doc_summary.unwrap_or_else(|| heuristic_purpose(path, language, &signatures))
    ));
    summary.push(format!(
        "Key exports/entrypoints: {}",
        if signatures.is_empty() {
            "No clear declarations detected".to_string()
        } else {
            signatures.join("; ")
        }
    ));
    summary.push(format!(
        "Important invariants: {}",
        heuristic_invariants(content, language)
    ));
    summary.push(format!(
        "Likely touch points: {}",
        heuristic_touch_points(&imports, &tests)
    ));
    summary.join("\n")
}

fn first_doc_line(content: &str, language: Language) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        if is_doc_comment(language, trimmed) {
            Some(
                trimmed
                    .trim_start_matches('/')
                    .trim_start_matches('*')
                    .trim()
                    .to_string(),
            )
        } else {
            None
        }
    })
}

fn heuristic_purpose(path: &Path, language: Language, signatures: &[String]) -> String {
    if let Some(first) = signatures.first() {
        return format!("Likely centered on {}", truncate(first, 80));
    }
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("content");
    match language {
        Language::Data => format!("Data or documentation for `{stem}`."),
        Language::Unknown => format!("Project artifact related to `{stem}`."),
        _ => format!("Defines code related to `{stem}`."),
    }
}

fn heuristic_invariants(content: &str, language: Language) -> String {
    let invariant = content.lines().find_map(|line| {
        let trimmed = line.trim();
        match language {
            Language::Rust
                if trimmed.starts_with("pub const ") || trimmed.starts_with("const ") =>
            {
                Some(trimmed.to_string())
            }
            Language::Go if trimmed.starts_with("const ") => Some(trimmed.to_string()),
            Language::TypeScript | Language::JavaScript
                if trimmed.starts_with("export const ") || trimmed.starts_with("const ") =>
            {
                Some(trimmed.to_string())
            }
            _ => None,
        }
    });
    invariant
        .map(|line| truncate(&line, 90))
        .unwrap_or_else(|| "No explicit invariants detected; inspect kept signatures.".to_string())
}

fn heuristic_touch_points(imports: &[String], tests: &[String]) -> String {
    if !imports.is_empty() {
        return imports.join("; ");
    }
    if !tests.is_empty() {
        return tests.join("; ");
    }
    "Nearby callers, tests, and module imports are the likely touch points.".to_string()
}
