use super::*;

#[test]
fn normalize_ripgrep_pattern_translates_grep_alternation() {
    assert_eq!(
        pattern::normalize_ripgrep_pattern("foo\\|bar\\|baz", &GrepOptions::default()),
        "foo|bar|baz"
    );
}

#[test]
fn normalize_ripgrep_pattern_preserves_literal_bre_parentheses() {
    assert_eq!(
        pattern::normalize_ripgrep_pattern(
            "ToJson(const VoiceConnectorSnapshot",
            &GrepOptions::default()
        ),
        "ToJson\\(const VoiceConnectorSnapshot"
    );
}

#[test]
fn normalize_ripgrep_pattern_preserves_literal_bre_pipe() {
    assert_eq!(
        pattern::normalize_ripgrep_pattern("foo|bar", &GrepOptions::default()),
        "foo\\|bar"
    );
}

#[test]
fn normalize_ripgrep_pattern_translates_bre_grouping_repetition_and_word_boundaries() {
    assert_eq!(
        pattern::normalize_ripgrep_pattern("\\<foo\\(bar\\)\\{2\\}\\+\\>", &GrepOptions::default()),
        "\\bfoo(bar){2}+\\b"
    );
}

#[test]
fn normalize_ripgrep_pattern_preserves_unmatched_escaped_open_paren_as_literal() {
    assert_eq!(
        pattern::normalize_ripgrep_pattern("add_library\\(", &GrepOptions::default()),
        "add_library\\("
    );
}

#[test]
fn normalize_ripgrep_pattern_preserves_fixed_strings() {
    assert_eq!(
        pattern::normalize_ripgrep_pattern(
            "foo\\|bar",
            &GrepOptions {
                fixed_strings: true,
                ..GrepOptions::default()
            }
        ),
        "foo\\|bar"
    );
}

#[test]
fn normalize_ripgrep_pattern_preserves_extended_regexp_patterns() {
    assert_eq!(
        pattern::normalize_ripgrep_pattern(
            "foo|bar|add_library\\(",
            &GrepOptions {
                extended_regexp: true,
                ..GrepOptions::default()
            }
        ),
        "foo|bar|add_library\\("
    );
}

#[test]
fn basic_alternation_hint_only_warns_for_unescaped_basic_pipe() {
    assert!(
        output::basic_alternation_hint(&["foo|bar".to_string()], &GrepOptions::default())
            .unwrap()
            .contains("bare `|`")
    );
    assert_eq!(
        output::basic_alternation_hint(&["foo\\|bar".to_string()], &GrepOptions::default()),
        None
    );
    assert_eq!(
        output::basic_alternation_hint(
            &["foo|bar".to_string()],
            &GrepOptions {
                extended_regexp: true,
                ..GrepOptions::default()
            }
        ),
        None
    );
    assert_eq!(
        output::basic_alternation_hint(
            &["foo|bar".to_string()],
            &GrepOptions {
                fixed_strings: true,
                ..GrepOptions::default()
            }
        ),
        None
    );
}
