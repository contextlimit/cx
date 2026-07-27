use std::ffi::OsStr;
use std::path::Path;

const DOCUMENT_EXTENSIONS: &[&str] = &[
    "adoc", "asc", "asciidoc", "creole", "csv", "diff", "docbook", "markdown", "md", "mdown",
    "mdx", "mkd", "org", "patch", "pod", "rdoc", "rest", "rst", "tex", "text", "textile", "tsv",
    "txt", "wiki",
];
const HUMAN_SOURCE_EXTENSIONS: &[&str] = &[
    "bash", "c", "cc", "cjs", "cmake", "comp", "cpp", "cs", "css", "cts", "cxx", "frag", "glsl",
    "go", "h", "hh", "hlsl", "hpp", "htm", "html", "hxx", "java", "js", "jsx", "kt", "kts", "less",
    "lua", "m", "metal", "mjs", "mm", "mts", "php", "pl", "py", "rb", "rs", "sass", "scss", "sh",
    "swift", "toml", "ts", "tsx", "vert", "wgsl", "yaml", "yml", "zsh",
];
pub fn is_document_path(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            DOCUMENT_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

pub fn is_exact_read_path(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    is_document_path(path) || is_compaction_protected_path(path)
}

pub fn is_human_source_path(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    !has_generated_ancestor(path)
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                HUMAN_SOURCE_EXTENSIONS
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            })
}

pub fn is_compaction_protected_path(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    is_control_instruction_path(path) || is_plan_json_path(path)
}

fn is_control_instruction_path(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("SKILL.md" | "AGENTS.md")
    )
}

fn is_plan_json_path(path: &Path) -> bool {
    let is_json = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    is_json && has_plans_ancestor(path)
}

fn has_plans_ancestor(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == OsStr::new("plans"))
}

fn has_generated_ancestor(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some("generated" | "codegen")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_human_document_extensions() {
        assert!(is_document_path("spec.md"));
        assert!(is_document_path("notes.MARKDOWN"));
        assert!(is_document_path("requirements.rst"));
        assert!(is_document_path("operator-guide.txt"));
        assert!(is_document_path("design.adoc"));
        assert!(is_document_path("exact-commit.diff"));
        assert!(is_document_path("series.PATCH"));
        assert!(is_document_path("audit.tsv"));
        assert!(is_document_path("report.CSV"));
    }

    #[test]
    fn does_not_treat_source_or_machine_data_as_documents() {
        assert!(!is_document_path("src/main.rs"));
        assert!(!is_document_path("package.json"));
        assert!(!is_document_path("events.log"));
        assert!(!is_document_path("Cargo.toml"));
    }

    #[test]
    fn protects_plan_json_and_control_instructions() {
        assert!(is_compaction_protected_path(
            "/Users/example/project4/.state/plans/38b40715962c7b9d/planSteps.json"
        ));
        assert!(is_compaction_protected_path(
            ".state/plans/38b40715962c7b9d/submit_payload.JSON"
        ));
        assert!(is_compaction_protected_path("SKILL.md"));
        assert!(is_compaction_protected_path("rules/AGENTS.md"));
    }

    #[test]
    fn ordinary_json_and_similar_plan_paths_remain_compactable() {
        assert!(!is_compaction_protected_path("package.json"));
        assert!(!is_compaction_protected_path("planSteps.json"));
        assert!(!is_compaction_protected_path(
            ".state/docs/example/planSteps.json"
        ));
        assert!(!is_compaction_protected_path(
            ".state/plan/example/planSteps.json"
        ));
        assert!(!is_compaction_protected_path(
            ".state/plans/example/planSteps.yaml"
        ));
        assert!(!is_compaction_protected_path(
            ".state/plans.json/example.json"
        ));
    }

    #[test]
    fn exact_reads_include_documents_and_protected_machine_data() {
        assert!(is_exact_read_path("spec.md"));
        assert!(is_exact_read_path("SKILL.md"));
        assert!(is_exact_read_path(
            ".state/plans/example/requirementLedger.json"
        ));
        assert!(!is_exact_read_path("package.json"));
    }

    #[test]
    fn recognizes_human_source_without_reclassifying_generated_data() {
        assert!(is_human_source_path("src/main.rs"));
        assert!(is_human_source_path("styles/panel.CSS"));
        assert!(is_human_source_path("packages/view.jsx"));
        assert!(is_human_source_path("CMake/module.cmake"));
        assert!(!is_human_source_path("package.json"));
        assert!(!is_human_source_path("events.log"));
        assert!(!is_human_source_path("generated/client.rs"));
        assert!(!is_human_source_path("sdk/codegen/bindings.cpp"));
    }
}
