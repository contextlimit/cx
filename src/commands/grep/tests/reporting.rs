use super::*;
use crate::commands::grep::reporting as search_reporting;

#[test]
fn command_family_preserves_invocation_and_selected_dialect() {
    let cases = [
        (
            vec!["cx".to_string(), "grep".to_string()],
            false,
            false,
            false,
            "grep basic",
        ),
        (
            vec!["cx".to_string(), "grep".to_string()],
            true,
            false,
            false,
            "grep extended",
        ),
        (
            vec!["cx".to_string(), "grep".to_string()],
            true,
            true,
            false,
            "grep fixed",
        ),
        (
            vec!["cx".to_string(), "rg".to_string()],
            true,
            false,
            false,
            "rg extended",
        ),
        (
            vec!["cx".to_string(), "--".to_string(), "rg".to_string()],
            true,
            true,
            false,
            "rg fixed",
        ),
        (
            vec!["cx".to_string(), "--".to_string(), "rg".to_string()],
            true,
            false,
            true,
            "rg files",
        ),
    ];

    for (raw_args, extended, fixed, files, expected) in cases {
        assert_eq!(
            search_reporting::command_family(&raw_args, extended, fixed, files),
            expected
        );
    }
}

#[test]
fn observation_source_exposes_execution_route_and_search_mode() {
    let context = search_reporting::observation_source(
        search_reporting::SearchBackend::Grep,
        search_reporting::SearchRoute::RgRegexRetry,
        &GrepOptions {
            extended_regexp: true,
            context_lines: Some(2),
            ..GrepOptions::default()
        },
        false,
        0,
        true,
        false,
    );
    assert_eq!(
        context,
        "search backend=grep route=rg-regex-retry dialect=extended mode=context result=success hint=none"
    );

    let no_match = search_reporting::observation_source(
        search_reporting::SearchBackend::Rg,
        search_reporting::SearchRoute::Preferred,
        &GrepOptions::default(),
        false,
        1,
        false,
        true,
    );
    assert_eq!(
        no_match,
        "search backend=rg route=preferred dialect=basic mode=matches result=no-match hint=extended-alternation"
    );

    let files = search_reporting::observation_source(
        search_reporting::SearchBackend::Walkdir,
        search_reporting::SearchRoute::RgUnavailable,
        &GrepOptions::default(),
        true,
        0,
        false,
        false,
    );
    assert_eq!(
        files,
        "search backend=walkdir route=rg-unavailable dialect=none mode=files result=no-match hint=none"
    );
}
