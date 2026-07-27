use super::GrepOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchBackend {
    Rg,
    Grep,
    Walkdir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchRoute {
    Preferred,
    RgUnavailable,
    RgRegexRetry,
}

pub(crate) fn command_family(
    raw_args: &[String],
    extended_regexp: bool,
    fixed_strings: bool,
    files: bool,
) -> String {
    let command = if invoked_as_rg(raw_args) {
        "rg"
    } else {
        "grep"
    };
    let mode = if files {
        "files"
    } else if fixed_strings {
        "fixed"
    } else if extended_regexp {
        "extended"
    } else {
        "basic"
    };
    format!("{command} {mode}")
}

pub(super) fn observation_source(
    backend: SearchBackend,
    route: SearchRoute,
    options: &GrepOptions,
    files: bool,
    exit_code: i32,
    has_results: bool,
    has_extended_alternation_hint: bool,
) -> String {
    format!(
        "search backend={} route={} dialect={} mode={} result={} hint={}",
        backend_name(backend),
        route_name(route),
        dialect_name(options, files),
        mode_name(options, files),
        result_name(exit_code, has_results),
        if has_extended_alternation_hint {
            "extended-alternation"
        } else {
            "none"
        }
    )
}

fn invoked_as_rg(raw_args: &[String]) -> bool {
    let mut args = raw_args.iter().map(String::as_str);
    if matches!(args.clone().next(), Some("cx")) {
        args.next();
    }
    if matches!(args.clone().next(), Some("--")) {
        args.next();
    }
    matches!(args.next(), Some("rg"))
}

fn backend_name(backend: SearchBackend) -> &'static str {
    match backend {
        SearchBackend::Rg => "rg",
        SearchBackend::Grep => "grep",
        SearchBackend::Walkdir => "walkdir",
    }
}

fn route_name(route: SearchRoute) -> &'static str {
    match route {
        SearchRoute::Preferred => "preferred",
        SearchRoute::RgUnavailable => "rg-unavailable",
        SearchRoute::RgRegexRetry => "rg-regex-retry",
    }
}

fn dialect_name(options: &GrepOptions, files: bool) -> &'static str {
    if files {
        "none"
    } else if options.fixed_strings {
        "fixed"
    } else if options.extended_regexp {
        "extended"
    } else {
        "basic"
    }
}

fn mode_name(options: &GrepOptions, files: bool) -> &'static str {
    if files {
        "files"
    } else if options.files_with_matches {
        "files-with-matches"
    } else if options.context_before.is_some()
        || options.context_after.is_some()
        || options.context_lines.is_some()
    {
        "context"
    } else {
        "matches"
    }
}

fn result_name(exit_code: i32, has_results: bool) -> &'static str {
    if exit_code > 1 {
        "error"
    } else if exit_code == 1 || !has_results {
        "no-match"
    } else {
        "success"
    }
}
