use crate::support::utils::shell_join;

const REDACTED: &str = "[REDACTED]";

const SECRET_PREFIXES: &[(&str, usize)] = &[
    ("sk-proj-", 8),
    ("sk-", 8),
    ("ghp_", 8),
    ("gho_", 8),
    ("ghu_", 8),
    ("github_pat_", 12),
    ("xoxb-", 8),
    ("xoxp-", 8),
    ("xoxa-", 8),
    ("AKIA", 12),
    ("ASIA", 12),
    ("AIza", 12),
    ("ya29.", 12),
    ("eyJ", 20),
];

pub fn redact_argv(args: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            redacted.push(redact_value(arg));
            redact_next = false;
            continue;
        }
        let (value, consumes_next) = redact_arg(arg);
        redacted.push(value);
        redact_next = consumes_next;
    }
    redacted
}

pub fn redacted_shell_join(args: &[String]) -> String {
    shell_join(&redact_argv(args))
}

pub(crate) fn telemetry_program_name(program: &str) -> Option<String> {
    let name = program
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(program);
    if name.is_empty()
        || name.len() > 80
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '+' | '-'))
        || has_common_secret_prefix(name)
        || redact_bearer(name).is_some()
        || is_secret_name(name)
    {
        return None;
    }

    if redact_value(name) != REDACTED || looks_like_structured_program_name(name) {
        Some(name.to_string())
    } else {
        None
    }
}

pub fn redact_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut token_start = None;
    for (index, ch) in value.char_indices() {
        if is_free_text_token_char(ch) {
            token_start.get_or_insert(index);
        } else {
            if let Some(start) = token_start.take() {
                output.push_str(&redact_arg(&value[start..index]).0);
            }
            output.push(ch);
        }
    }
    if let Some(start) = token_start {
        output.push_str(&redact_arg(&value[start..]).0);
    }
    output
}

pub fn argument_shape_join(args: &[String]) -> String {
    shell_join(&argument_shape(args))
}

pub fn stable_shape_hash(shape: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in shape.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn argument_shape(args: &[String]) -> Vec<String> {
    let mut redacted = redact_argv(args);
    if let (Some(program), Some(shaped_program)) = (args.first(), redacted.first_mut()) {
        *shaped_program = telemetry_program_name(program).unwrap_or_else(|| REDACTED.to_string());
    }
    let program = redacted.first().map(String::as_str).unwrap_or_default();
    redacted
        .iter()
        .enumerate()
        .map(|(index, arg)| shape_arg(program, index, arg))
        .collect()
}

fn shape_arg(program: &str, index: usize, arg: &str) -> String {
    if index == 0 {
        return if arg == REDACTED {
            "<redacted>".to_string()
        } else {
            arg.to_string()
        };
    }
    if is_preserved_subcommand(program, index, arg) {
        return arg.to_string();
    }
    if arg == "--" {
        return arg.to_string();
    }
    if let Some((flag, value)) = arg.strip_prefix("--").and_then(|arg| arg.split_once('=')) {
        return format!("--{flag}={}", shape_value(value));
    }
    if arg.starts_with('-') {
        return arg.to_string();
    }
    shape_value(arg)
}

fn is_preserved_subcommand(program: &str, index: usize, arg: &str) -> bool {
    index == 1
        && !arg.starts_with('-')
        && matches!(
            program,
            "git" | "cargo" | "go" | "docker" | "kubectl" | "cmake" | "node"
        )
}

fn shape_value(value: &str) -> String {
    if value == REDACTED {
        return "<redacted>".to_string();
    }
    if value.is_empty() {
        return "<empty>".to_string();
    }
    if value.contains('\n') || value.contains('\r') {
        return "<script>".to_string();
    }
    if looks_like_line_range(value) {
        return "<range>".to_string();
    }
    if looks_like_number(value) {
        return "<number>".to_string();
    }
    if looks_like_revision_range(value) {
        return "<rev-range>".to_string();
    }
    if looks_like_revision(value) {
        return "<rev>".to_string();
    }
    if looks_like_path(value) {
        return path_shape(value);
    }
    if looks_like_glob(value) {
        return "<glob>".to_string();
    }
    if looks_like_pattern(value) {
        return "<pattern>".to_string();
    }
    if value.len() > 40 {
        return "<text>".to_string();
    }
    "<value>".to_string()
}

fn looks_like_line_range(value: &str) -> bool {
    let Some((start, end)) = value.split_once(':') else {
        return false;
    };
    (!start.is_empty() || !end.is_empty())
        && start.chars().all(|ch| ch.is_ascii_digit())
        && end.chars().all(|ch| ch.is_ascii_digit())
}

fn looks_like_number(value: &str) -> bool {
    value
        .strip_prefix('-')
        .unwrap_or(value)
        .chars()
        .all(|ch| ch.is_ascii_digit())
}

fn looks_like_revision_range(value: &str) -> bool {
    value.contains("..") && !value.contains(['/', '\\', ' ', '\t'])
}

fn looks_like_revision(value: &str) -> bool {
    matches!(value, "HEAD" | "HEAD^" | "HEAD~1")
        || value
            .strip_prefix("HEAD~")
            .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
        || (value.len() >= 7 && value.len() <= 40 && value.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn looks_like_path(value: &str) -> bool {
    value.contains(['/', '\\'])
        || matches!(value, "." | "..")
        || value.starts_with("./")
        || value.starts_with("../")
        || value.rsplit_once('.').is_some_and(|(stem, extension)| {
            !stem.is_empty() && is_common_path_extension(extension)
        })
}

fn path_shape(value: &str) -> String {
    value
        .rsplit_once('.')
        .map(|(_, extension)| extension.trim_matches(|ch: char| !ch.is_ascii_alphanumeric()))
        .filter(|extension| is_common_path_extension(extension))
        .map(|extension| format!("<path:{extension}>"))
        .unwrap_or_else(|| "<path>".to_string())
}

fn is_common_path_extension(extension: &str) -> bool {
    matches!(
        extension,
        "c" | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "m"
            | "mm"
            | "rs"
            | "go"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "md"
            | "txt"
            | "py"
            | "sh"
            | "sql"
            | "html"
            | "css"
            | "scss"
            | "png"
            | "jpg"
            | "jpeg"
            | "svg"
    )
}

fn looks_like_glob(value: &str) -> bool {
    value.contains(['*', '?']) || (value.contains('[') && value.contains(']'))
}

fn looks_like_pattern(value: &str) -> bool {
    value.contains(['|', '(', ')', '{', '}', '+', '^', '$'])
}

fn redact_arg(arg: &str) -> (String, bool) {
    if let Some(redacted) = redact_long_flag_assignment(arg) {
        return (redacted, false);
    }
    if let Some(redacted) = redact_assignment(arg) {
        return (redacted, false);
    }
    if is_secret_flag(arg) {
        return (arg.to_string(), true);
    }
    (redact_value(arg), false)
}

fn is_free_text_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+' | '=')
}

fn redact_long_flag_assignment(arg: &str) -> Option<String> {
    let (name, value) = arg.strip_prefix("--")?.split_once('=')?;
    if is_secret_name(name) {
        Some(format!("--{name}={REDACTED}"))
    } else {
        Some(format!("--{name}={}", redact_value(value))).filter(|redacted| redacted != arg)
    }
}

fn redact_assignment(arg: &str) -> Option<String> {
    let (name, value) = arg.split_once('=')?;
    if name.is_empty() || name.starts_with('-') || name.contains(['/', '\\']) {
        return None;
    }
    if is_secret_name(name) {
        Some(format!("{name}={REDACTED}"))
    } else {
        Some(format!("{name}={}", redact_value(value))).filter(|redacted| redacted != arg)
    }
}

fn is_secret_flag(arg: &str) -> bool {
    arg.strip_prefix("--").is_some_and(is_secret_name)
}

fn is_secret_name(name: &str) -> bool {
    let lower = name.trim_start_matches('-').to_ascii_lowercase();
    let compact = lower.replace(['-', '_'], "");
    let sensitive_exact = [
        "apikey",
        "accesskey",
        "authkey",
        "clientsecret",
        "privkey",
        "privatekey",
        "sshkey",
    ];
    if sensitive_exact.contains(&compact.as_str()) {
        return true;
    }
    if matches!(lower.as_str(), "key" | "pwd" | "passwd") {
        return true;
    }
    lower.split(['-', '_', '.']).any(|part| {
        matches!(
            part,
            "token"
                | "secret"
                | "password"
                | "auth"
                | "authorization"
                | "bearer"
                | "credential"
                | "credentials"
        )
    })
}

fn redact_value(value: &str) -> String {
    if let Some(redacted) = redact_bearer(value) {
        return redacted;
    }
    if looks_like_secret_value(value) {
        REDACTED.to_string()
    } else {
        value.to_string()
    }
}

fn redact_bearer(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let marker = "bearer ";
    let index = lower.find(marker)?;
    let token_start = index + marker.len();
    if value[token_start..].trim().is_empty() {
        return None;
    }
    Some(format!("{}{}", &value[..token_start], REDACTED))
}

fn looks_like_secret_value(value: &str) -> bool {
    let trimmed = value.trim_matches(|ch: char| matches!(ch, '\'' | '"' | ',' | ';'));
    has_common_secret_prefix(trimmed)
        || path_contains_secret_component(trimmed)
        || looks_like_random_key(trimmed)
}

fn path_contains_secret_component(value: &str) -> bool {
    if !value.contains(['/', '\\']) {
        return false;
    }
    let mut basename = "";
    for component in value
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
    {
        if has_common_secret_prefix(component) {
            return true;
        }
        basename = component;
    }
    looks_like_random_key(basename) && !looks_like_structured_program_name(basename)
}

fn has_common_secret_prefix(value: &str) -> bool {
    SECRET_PREFIXES.iter().any(|(prefix, min_suffix)| {
        value
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
            && value.len() >= prefix.len() + min_suffix
    })
}

fn looks_like_random_key(value: &str) -> bool {
    if value.len() < 16 || value.contains(['/', '\\', ' ', '\t', '\n', '\r', '|', '*']) {
        return false;
    }
    if looks_like_human_readable_identifier(value) {
        return false;
    }
    if value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return false;
    }
    let classes = char_class_count(value);
    let unique = unique_ascii_count(value);
    let alnum = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .count();
    alnum >= 12 && classes >= 3 && unique >= 10
}

fn looks_like_human_readable_identifier(value: &str) -> bool {
    if is_secret_name(value) {
        return false;
    }
    let parts = value
        .split(['.', '_', '+', '-'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 3 {
        return false;
    }

    let mut word_parts = 0usize;
    let mut vowel_parts = 0usize;
    for part in parts {
        if is_version_part(part) {
            continue;
        }
        if !is_word_cased_identifier_part(part) {
            return false;
        }
        word_parts += 1;
        if part
            .chars()
            .any(|ch| matches!(ch.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y'))
        {
            vowel_parts += 1;
        }
    }
    word_parts >= 3 && vowel_parts >= 2
}

fn is_version_part(value: &str) -> bool {
    value.chars().all(|ch| ch.is_ascii_digit())
        || value.strip_prefix('v').is_some_and(|version| {
            !version.is_empty() && version.chars().all(|ch| ch.is_ascii_digit())
        })
}

fn is_word_cased_identifier_part(value: &str) -> bool {
    if value.len() < 2 || value.len() > 24 || !value.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    let lower = value.chars().all(|ch| ch.is_ascii_lowercase());
    let short_upper = value.len() <= 5 && value.chars().all(|ch| ch.is_ascii_uppercase());
    let title = value
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
        && value.chars().skip(1).all(|ch| ch.is_ascii_lowercase());
    lower || short_upper || title
}

fn looks_like_structured_program_name(value: &str) -> bool {
    if value
        .chars()
        .any(|ch| ch.is_ascii_uppercase() || !ch.is_ascii())
    {
        return false;
    }
    let separator_count = value
        .chars()
        .filter(|ch| matches!(ch, '.' | '_' | '+' | '-'))
        .count();
    let word_count = value
        .split(['.', '_', '+', '-'])
        .filter(|part| part.len() >= 2 && part.chars().all(|ch| ch.is_ascii_lowercase()))
        .count();
    let has_version_segment = value.split(['.', '_', '+', '-']).any(|part| {
        part.strip_prefix('v').is_some_and(|version| {
            !version.is_empty() && version.chars().all(|ch| ch.is_ascii_digit())
        })
    });
    separator_count >= 2 && word_count >= 2 && has_version_segment
}

fn char_class_count(value: &str) -> usize {
    [
        value.chars().any(|ch| ch.is_ascii_lowercase()),
        value.chars().any(|ch| ch.is_ascii_uppercase()),
        value.chars().any(|ch| ch.is_ascii_digit()),
        value
            .chars()
            .any(|ch| matches!(ch, '_' | '-' | '.' | '+' | '/' | '=')),
    ]
    .into_iter()
    .filter(|present| *present)
    .count()
}

fn unique_ascii_count(value: &str) -> usize {
    let mut seen = [false; 128];
    for byte in value.bytes().filter(|byte| byte.is_ascii()) {
        seen[usize::from(byte)] = true;
    }
    seen.into_iter().filter(|present| *present).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn redacts_secret_flag_values_and_assignments() {
        let redacted = redact_argv(&strings(&[
            "cx",
            "grep",
            "--api-key",
            "sk-proj-abcdefghijklmnopqrstuvwxyz",
            "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz",
            concat!("--client-secret=", "abcDEF1234567890"),
        ]));
        assert_eq!(
            redacted,
            strings(&[
                "cx",
                "grep",
                "--api-key",
                REDACTED,
                "OPENAI_API_KEY=[REDACTED]",
                "--client-secret=[REDACTED]",
            ])
        );
    }

    #[test]
    fn redacts_secret_values_embedded_in_free_text() {
        let text = "fixed token=sk-abcdefghijklmnopqrstuvwxyz after bearer abcXYZ1234567890";
        assert_eq!(
            redact_text(text),
            "fixed token=[REDACTED] after bearer [REDACTED]"
        );
        assert_eq!(
            redact_text("revision r111 and commit 0123456789abcdef"),
            "revision r111 and commit 0123456789abcdef"
        );
    }

    #[test]
    fn redacts_common_provider_prefixes_and_bearer_headers() {
        let redacted = redact_argv(&strings(&[
            "Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz",
            "SK-PROJ-ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "github_pat_abcdefghijklmnopqrstuvwxyz123456",
            "AKIAIOSFODNN7EXAMPLE",
            "AIzaSyAabcdefghijklmnopqrstuvwxyz",
            "ya29.a0ARrdaMabcdefghijklmnopqrstuvwxyz",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
        ]));
        assert_eq!(
            redacted,
            strings(&[
                "Authorization: Bearer [REDACTED]",
                REDACTED,
                REDACTED,
                REDACTED,
                REDACTED,
                REDACTED,
                REDACTED,
            ])
        );
    }

    #[test]
    fn redacts_high_entropy_key_like_values_without_hiding_paths_or_regexes() {
        let redacted = redact_argv(&strings(&[
            "A9qZ_8LmNoP2rStUv",
            "/tmp/SK-PROJ-ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "src/support/insights.rs",
            "route|path|Dashboard",
            "7354c8ed0f94e0f0e5e5e5e5e5e5e5e5e5e5e5e5",
            "feature/add-command-argv",
        ]));
        assert_eq!(redacted[0], REDACTED);
        assert_eq!(redacted[1], REDACTED);
        assert_eq!(redacted[2], "src/support/insights.rs");
        assert_eq!(redacted[3], "route|path|Dashboard");
        assert_eq!(redacted[4], "7354c8ed0f94e0f0e5e5e5e5e5e5e5e5e5e5e5e5");
        assert_eq!(redacted[5], "feature/add-command-argv");
    }

    #[test]
    fn preserves_human_readable_build_targets_without_weakening_secret_redaction() {
        let redacted = redact_argv(&strings(&[
            "cmake",
            "--build",
            "build-web",
            "--target",
            "sample-web-service",
            "sample-ui-renderer-assets",
            "authorization-token-v1",
            "abcdefghij-klmnop-123456",
        ]));
        assert_eq!(redacted[4], "sample-web-service");
        assert_eq!(redacted[5], "sample-ui-renderer-assets");
        assert_eq!(redacted[6], REDACTED);
        assert_eq!(redacted[7], REDACTED);
    }

    #[test]
    fn redacted_shell_join_quotes_after_redaction() {
        let joined = redacted_shell_join(&strings(&[
            "cx",
            "report",
            "cx",
            "grep",
            "--token",
            "xoxb-abcdefghijklmnopqrstuvwxyz",
        ]));
        assert_eq!(joined, "cx report cx grep --token '[REDACTED]'");
    }

    #[test]
    fn telemetry_program_names_preserve_structured_tools_without_exposing_secrets() {
        assert_eq!(
            telemetry_program_name(
                "/Users/example/project/app/sample-suite/build/sample-suite-tests"
            )
            .as_deref(),
            Some("sample-suite-tests")
        );
        assert_eq!(
            telemetry_program_name(r"C:\work\bin\inventory_host_v2.sh").as_deref(),
            Some("inventory_host_v2.sh")
        );
        assert_eq!(
            telemetry_program_name("clang-format").as_deref(),
            Some("clang-format")
        );
        assert_eq!(
            telemetry_program_name("/tmp/sk-proj-abcdefghijklmnopqrstuvwxyz"),
            None
        );
        assert_eq!(telemetry_program_name("A9qZ_8LmNoP2rStUv"), None);
        assert_eq!(
            telemetry_program_name("/tmp/abcdefghij-klmnop-123456"),
            None
        );
        assert_eq!(telemetry_program_name("authorization-token-v1"), None);
    }

    #[test]
    fn argument_shape_hides_paths_patterns_and_secret_values() {
        let shaped = argument_shape_join(&strings(&[
            "git",
            "diff",
            "--no-color",
            "HEAD^..HEAD",
            "--",
            "sdk/cx_sdk_dev/src/platform/web/app_web_service_desk_api.hpp",
        ]));
        assert_eq!(shaped, "git diff --no-color '<rev-range>' -- '<path:hpp>'");

        let grep = argument_shape_join(&strings(&[
            "rg",
            "-n",
            "route|path|Dashboard",
            "app/tests/example.mjs",
            "--token",
            "sk-proj-abcdefghijklmnopqrstuvwxyz",
        ]));
        assert_eq!(grep, "rg -n '<pattern>' '<path:mjs>' --token '<redacted>'");
    }

    #[test]
    fn argument_shape_normalizes_program_paths_and_rejected_program_values() {
        let relative = argument_shape_join(&strings(&[
            "app/build/sample-suite-tests",
            "--gtest_filter=Planner.*",
        ]));
        let absolute = argument_shape_join(&strings(&[
            "/Users/example/project/app/build/sample-suite-tests",
            "--gtest_filter=Planner.*",
        ]));
        assert_eq!(relative, "sample-suite-tests '--gtest_filter=<glob>'");
        assert_eq!(relative, absolute);
        assert_eq!(stable_shape_hash(&relative), stable_shape_hash(&absolute));

        let secret = argument_shape_join(&strings(&[
            "/tmp/sk-proj-abcdefghijklmnopqrstuvwxyz",
            "--version",
        ]));
        assert_eq!(secret, "'<redacted>' --version");
        assert!(!secret.contains("sk-proj-"));
    }

    #[test]
    fn stable_shape_hash_is_repeatable() {
        let shape = "git diff '<rev-range>'";
        assert_eq!(stable_shape_hash(shape), stable_shape_hash(shape));
        assert_ne!(
            stable_shape_hash(shape),
            stable_shape_hash("git status --short")
        );
    }
}
