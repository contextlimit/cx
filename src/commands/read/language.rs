use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Shell,
    Data,
    Unknown,
}

impl Language {
    pub(super) fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
        {
            "rs" => Self::Rust,
            "py" => Self::Python,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "tsx" => Self::TypeScript,
            "go" => Self::Go,
            "sh" | "bash" | "zsh" => Self::Shell,
            "json" | "jsonc" | "yaml" | "yml" | "toml" | "xml" | "csv" | "md" | "txt" | "lock" => {
                Self::Data
            }
            _ => Self::Unknown,
        }
    }

    pub(super) fn as_smart_label(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Go => "go",
            Self::Shell => "shell",
            Self::Data => "data",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) fn is_doc_comment(language: Language, trimmed: &str) -> bool {
    match language {
        Language::Rust => trimmed.starts_with("///") || trimmed.starts_with("//!"),
        Language::Python | Language::Shell => false,
        _ => trimmed.starts_with("/**") || trimmed.starts_with("/*") || trimmed.starts_with('*'),
    }
}

pub(super) fn is_attribute_line(language: Language, trimmed: &str) -> bool {
    match language {
        Language::Rust => trimmed.starts_with("#["),
        Language::Python => trimmed.starts_with('@'),
        _ => false,
    }
}

pub(super) fn should_keep_signature(language: Language, trimmed: &str) -> bool {
    is_import_line(language, trimmed)
        || is_declaration_line(language, trimmed)
        || looks_like_test_line(language, trimmed)
}

pub(super) fn is_import_line(language: Language, trimmed: &str) -> bool {
    match language {
        Language::Rust => {
            trimmed.starts_with("use ")
                || trimmed.starts_with("pub use ")
                || trimmed.starts_with("mod ")
                || trimmed.starts_with("pub mod ")
        }
        Language::Python => trimmed.starts_with("import ") || trimmed.starts_with("from "),
        Language::JavaScript | Language::TypeScript => {
            trimmed.starts_with("import ")
                || trimmed.starts_with("export ")
                || trimmed.starts_with("export default ")
        }
        Language::Go => trimmed.starts_with("import ") || trimmed == "import (",
        Language::Shell => trimmed.starts_with("source ") || trimmed.starts_with(". "),
        Language::Data | Language::Unknown => false,
    }
}

pub(super) fn is_declaration_line(language: Language, trimmed: &str) -> bool {
    match language {
        Language::Rust => matches_any_prefix(
            trimmed,
            &[
                "fn ",
                "pub fn ",
                "async fn ",
                "pub async fn ",
                "struct ",
                "pub struct ",
                "enum ",
                "pub enum ",
                "trait ",
                "pub trait ",
                "impl ",
                "pub const ",
                "const ",
                "pub static ",
                "static ",
                "type ",
                "pub type ",
            ],
        ),
        Language::Python => matches_any_prefix(trimmed, &["def ", "async def ", "class "]),
        Language::JavaScript | Language::TypeScript => matches_any_prefix(
            trimmed,
            &[
                "function ",
                "async function ",
                "export function ",
                "export async function ",
                "class ",
                "export class ",
                "interface ",
                "export interface ",
                "type ",
                "export type ",
                "const ",
                "export const ",
            ],
        ),
        Language::Go => {
            matches_any_prefix(trimmed, &["func ", "type ", "const ", "var ", "type ("])
        }
        Language::Shell => matches_any_prefix(trimmed, &["function "]) || trimmed.ends_with("() {"),
        Language::Data | Language::Unknown => false,
    }
}

pub(super) fn looks_like_test_line(language: Language, trimmed: &str) -> bool {
    match language {
        Language::Rust => trimmed == "#[test]" || trimmed.starts_with("fn test_"),
        Language::Python => trimmed.starts_with("def test_"),
        Language::JavaScript | Language::TypeScript => {
            trimmed.starts_with("test(")
                || trimmed.starts_with("it(")
                || trimmed.starts_with("describe(")
        }
        Language::Go => trimmed.starts_with("func Test"),
        _ => false,
    }
}

fn matches_any_prefix(value: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| value.starts_with(prefix))
}
