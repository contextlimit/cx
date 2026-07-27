use std::ffi::{OsStr, OsString};

use clap::error::ErrorKind;
use clap::Parser;

use super::{Cli, Command, GitProxyCommand};

pub fn parse_from_cx_args<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    match try_parse_collected_args(&args) {
        Ok(cli) => cli,
        Err(error) => {
            record_routing_rejection(&args, error.kind());
            error.exit()
        }
    }
}

pub fn try_parse_from_cx_args<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    try_parse_collected_args(&args)
}

fn try_parse_collected_args(args: &[OsString]) -> Result<Cli, clap::Error> {
    if has_explicit_separator(args) {
        return try_parse_explicit_auto_args(args);
    }
    match try_parse_supported_args(args, true) {
        Ok(cli) => Ok(cli),
        Err(error) => passthrough_after_parse_error_or_return(error, args),
    }
}

fn try_parse_explicit_auto_args(args: &[OsString]) -> Result<Cli, clap::Error> {
    if let Some(cli) = explicit_bash_args(args) {
        return Ok(cli);
    }
    let supported_args = args_without_explicit_separator(args);
    match try_parse_supported_args(&supported_args, false) {
        Ok(cli) if explicit_auto_command_is_clear(&cli.command) => Ok(cli),
        Ok(_) => passthrough_explicit_args_or_error(args),
        Err(error) => {
            if should_retry_explicit_grep_pattern_preprocess(&supported_args) {
                if let Ok(cli) = try_parse_supported_args(&supported_args, true) {
                    if explicit_auto_command_is_clear(&cli.command) {
                        return Ok(cli);
                    }
                }
            }
            if should_passthrough_explicit_parse_error(error.kind(), &supported_args) {
                passthrough_explicit_args_or_error(args)
            } else {
                Err(error)
            }
        }
    }
}

fn should_passthrough_explicit_parse_error(kind: ErrorKind, args: &[OsString]) -> bool {
    let Some(root) = args.get(1).and_then(|arg| arg.to_str()) else {
        return false;
    };
    if !is_known_cx_subcommand(root) {
        return true;
    }
    if explicit_auto_root_is_cx_owned(root) {
        return false;
    }
    matches!(
        kind,
        ErrorKind::InvalidSubcommand | ErrorKind::UnknownArgument
    )
}

fn explicit_auto_root_is_cx_owned(root: &str) -> bool {
    matches!(root, "read" | "report" | "insights" | "sh")
}

fn record_routing_rejection(args: &[OsString], kind: ErrorKind) {
    let Some(error_kind) = routing_error_kind_label(kind) else {
        return;
    };
    let passthrough_enabled = passthrough_enabled();
    let passthrough_eligible = parse_error_passthrough_eligible(kind, args);
    let reason = routing_rejection_reason(args, passthrough_eligible, passthrough_enabled);
    let normalized_args = normalized_raw_args(args);
    let record = crate::support::insights::RoutingDecisionRecord {
        args: &normalized_args,
        reason,
        error_kind,
        explicit_auto: has_explicit_separator(args),
        passthrough_eligible,
        passthrough_enabled,
    };
    if let Err(error) = crate::support::insights::record_routing_rejection(&record) {
        if std::env::var("CX_INSIGHTS_DEBUG").is_ok() {
            eprintln!("cx routing insights: {error:#}");
        }
    }
}

fn parse_error_passthrough_eligible(kind: ErrorKind, args: &[OsString]) -> bool {
    if has_explicit_separator(args) {
        return should_passthrough_explicit_parse_error(
            kind,
            &args_without_explicit_separator(args),
        );
    }
    should_passthrough_after_parse_error(kind, args)
}

fn routing_rejection_reason(
    args: &[OsString],
    passthrough_eligible: bool,
    passthrough_enabled: bool,
) -> &'static str {
    if passthrough_eligible {
        return if passthrough_enabled {
            "passthrough-unavailable"
        } else {
            "passthrough-disabled"
        };
    }
    let root_index = if has_explicit_separator(args) { 2 } else { 1 };
    let root = args.get(root_index).and_then(|arg| arg.to_str());
    if root.is_some_and(explicit_auto_root_is_cx_owned) {
        "cx-owned-parse-error"
    } else {
        "official-parse-error"
    }
}

fn routing_error_kind_label(kind: ErrorKind) -> Option<&'static str> {
    match kind {
        ErrorKind::InvalidValue => Some("invalid-value"),
        ErrorKind::UnknownArgument => Some("unknown-argument"),
        ErrorKind::InvalidSubcommand => Some("invalid-subcommand"),
        ErrorKind::NoEquals => Some("missing-equals"),
        ErrorKind::ValueValidation => Some("value-validation"),
        ErrorKind::TooManyValues => Some("too-many-values"),
        ErrorKind::TooFewValues => Some("too-few-values"),
        ErrorKind::WrongNumberOfValues => Some("wrong-number-of-values"),
        ErrorKind::ArgumentConflict => Some("argument-conflict"),
        ErrorKind::MissingRequiredArgument => Some("missing-required-argument"),
        ErrorKind::MissingSubcommand => Some("missing-subcommand"),
        ErrorKind::InvalidUtf8 => Some("invalid-utf8"),
        _ => None,
    }
}

fn explicit_auto_command_is_clear(command: &Command) -> bool {
    match command {
        Command::Diff { .. } => false,
        Command::Find { args } => find_args_are_clear(args),
        Command::Node { args } => node_args_are_clear(args),
        _ => true,
    }
}

fn find_args_are_clear(args: &[String]) -> bool {
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "(" | ")" | "-o" | "-or" | "-a" | "-and" => return false,
            "--" | "-print" | "-print0" | "--hidden" => {
                index += 1;
            }
            "-maxdepth" | "--maxdepth" | "--max-depth" | "-type" | "--type" | "-perm"
            | "--perm" | "-name" | "--name" | "-iname" | "--iname" | "-path" | "--path"
            | "-wholename" | "--wholename" | "-ipath" | "--ipath" | "-iwholename"
            | "--iwholename" | "--max-results" | "--head" => {
                index += 2;
            }
            _ if arg.starts_with("--max-results=")
                || arg.starts_with("--head=")
                || arg.starts_with("--maxdepth=")
                || arg.starts_with("--max-depth=") =>
            {
                index += 1;
            }
            _ if arg.starts_with('-') => return false,
            _ => {
                index += 1;
            }
        }
    }
    true
}

fn node_args_are_clear(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        None => true,
        Some("run") => args.len() > 1,
        Some("--input-type") => args.len() > 1,
        Some(arg) if arg.starts_with("--input-type=") => true,
        Some("test") => true,
        _ => node_check_args_are_clear(args),
    }
}

fn node_check_args_are_clear(args: &[String]) -> bool {
    if !crate::commands::node_cmd::check_flag_precedes_program(args) {
        return false;
    }
    let mut check_flag_seen = false;
    let mut file_seen = false;
    let mut after_separator = false;
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if after_separator {
            file_seen = true;
            index += 1;
            continue;
        }

        match arg.as_str() {
            "--" => {
                after_separator = true;
                index += 1;
            }
            "--check" | "-c" => {
                check_flag_seen = true;
                index += 1;
            }
            "--experimental-loader" | "--loader" => {
                index += 1;
                if args.get(index).is_none() {
                    return false;
                }
                index += 1;
            }
            value
                if value.starts_with("--experimental-loader=")
                    || value.starts_with("--loader=") =>
            {
                index += 1;
            }
            value if value.starts_with('-') => return false,
            _ => {
                file_seen = true;
                index += 1;
            }
        }
    }

    check_flag_seen && file_seen
}

fn passthrough_explicit_args_or_error(args: &[OsString]) -> Result<Cli, clap::Error> {
    if passthrough_enabled() {
        if let Some(passthrough_args) = passthrough_args(args) {
            return Ok(Cli {
                raw_args: normalized_raw_args(args),
                command: Command::Passthrough {
                    args: passthrough_args,
                },
            });
        }
    }
    Err(clap::Error::raw(
        ErrorKind::InvalidSubcommand,
        "unsupported command passthrough is disabled; enable with `cx insights settings --set passthrough_unsupported_commands=true`",
    ))
}

fn explicit_bash_args(args: &[OsString]) -> Option<Cli> {
    if !matches!(args.get(1), Some(value) if value == OsStr::new("--")) {
        return None;
    }
    if !matches!(args.get(2).and_then(|arg| arg.to_str()), Some("bash")) {
        return None;
    }
    let mut shell_args = args
        .get(3..)?
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let no_compact = shell_args.first().is_some_and(|arg| arg == "--no-compact");
    if no_compact {
        shell_args.remove(0);
    }
    if shell_args.is_empty() && !no_compact {
        return None;
    }
    Some(Cli {
        raw_args: normalized_raw_args(args),
        command: Command::Sh {
            no_compact,
            args: shell_args,
        },
    })
}

fn try_parse_supported_args(
    args: &[OsString],
    allow_grep_pattern_preprocess: bool,
) -> Result<Cli, clap::Error> {
    let preprocessed = preprocess_args(args.to_vec(), allow_grep_pattern_preprocess);
    let mut cli = Cli::try_parse_from(preprocessed)?;
    restore_git_pathspec_separator(&mut cli, args);
    cli.raw_args = normalized_raw_args(args);
    Ok(cli)
}

fn restore_git_pathspec_separator(cli: &mut Cli, raw_args: &[OsString]) {
    match &mut cli.command {
        Command::Diff { args } => restore_trailing_separator(args, raw_args, &["diff"]),
        Command::Git { command } => match command {
            GitProxyCommand::Status { args } => {
                restore_trailing_separator(args, raw_args, &["git", "status"]);
            }
            GitProxyCommand::Diff { args } => {
                restore_trailing_separator(args, raw_args, &["git", "diff"]);
            }
            GitProxyCommand::Log { args } => {
                restore_trailing_separator(args, raw_args, &["git", "log"]);
            }
            GitProxyCommand::Show { args } => {
                restore_trailing_separator(args, raw_args, &["git", "show"]);
            }
            GitProxyCommand::EvidenceDiff { args } => {
                restore_trailing_separator(args, raw_args, &["git", "evidence-diff"]);
            }
            GitProxyCommand::ConflictDiff { args } => {
                restore_trailing_separator(args, raw_args, &["git", "conflict-diff"]);
            }
        },
        _ => {}
    }
}

fn restore_trailing_separator(
    parsed_args: &mut Vec<String>,
    raw_args: &[OsString],
    command_path: &[&str],
) {
    if parsed_args.iter().any(|arg| arg == "--") {
        return;
    }
    let args_start = 1 + command_path.len();
    if raw_args.len() < args_start
        || command_path.iter().enumerate().any(|(index, expected)| {
            raw_args.get(index + 1).and_then(|arg| arg.to_str()) != Some(*expected)
        })
    {
        return;
    }
    let Some(separator_offset) = raw_args[args_start..]
        .iter()
        .position(|arg| arg == OsStr::new("--"))
    else {
        return;
    };
    parsed_args.insert(separator_offset.min(parsed_args.len()), "--".to_string());
}

fn passthrough_after_parse_error_or_return(
    error: clap::Error,
    args: &[OsString],
) -> Result<Cli, clap::Error> {
    if passthrough_enabled() && should_passthrough_after_parse_error(error.kind(), args) {
        if let Some(passthrough_args) = passthrough_args(args) {
            return Ok(Cli {
                raw_args: normalized_raw_args(args),
                command: Command::Passthrough {
                    args: passthrough_args,
                },
            });
        }
    }
    Err(error)
}

fn has_explicit_separator(args: &[OsString]) -> bool {
    matches!(args.get(1), Some(value) if value == OsStr::new("--"))
}

fn args_without_explicit_separator(args: &[OsString]) -> Vec<OsString> {
    let mut stripped = Vec::with_capacity(args.len().saturating_sub(1));
    if let Some(program) = args.first() {
        stripped.push(program.clone());
    }
    if let Some(rest) = args.get(2..) {
        stripped.extend(rest.iter().cloned());
    }
    stripped
}

fn normalized_raw_args(args: &[OsString]) -> Vec<String> {
    let mut raw = args
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if let Some(program) = raw.first_mut() {
        *program = "cx".to_string();
    }
    raw
}

fn preprocess_args(args: Vec<OsString>, allow_grep_pattern_preprocess: bool) -> Vec<OsString> {
    let args = preprocess_cmake_build_invocation(args);
    let args = preprocess_rg_invocation(args);
    let args = preprocess_repeated_grep_boolean_flags(args);
    if allow_grep_pattern_preprocess {
        preprocess_grep_pattern_args(args)
    } else {
        args
    }
}

fn preprocess_cmake_build_invocation(mut args: Vec<OsString>) -> Vec<OsString> {
    if args.len() >= 3
        && matches_subcommand(&args[1], &["cmake"])
        && matches_subcommand(&args[2], &["--build"])
    {
        args[2] = OsString::from("build");
    }
    args
}

fn preprocess_rg_invocation(mut args: Vec<OsString>) -> Vec<OsString> {
    if args.len() < 2 || !matches_subcommand(&args[1], &["rg"]) {
        return args;
    }

    let already_set = args.iter().skip(2).any(|arg| {
        matches!(
            arg.to_str(),
            Some("-E" | "--extended-regexp" | "--fixed-strings" | "-F")
        )
    });
    if already_set {
        return args;
    }

    args.insert(2, OsString::from("--extended-regexp"));
    args
}

fn preprocess_repeated_grep_boolean_flags(args: Vec<OsString>) -> Vec<OsString> {
    if args.len() < 2 || !matches_subcommand(&args[1], &["grep", "rg"]) {
        return args;
    }

    let mut normalized = args[..2].to_vec();
    let mut seen = 0u16;
    let mut expects_value = false;
    let mut after_separator = false;
    for arg in args.into_iter().skip(2) {
        let token = arg.to_str();
        if after_separator {
            normalized.push(arg);
            continue;
        }
        if expects_value {
            normalized.push(arg);
            expects_value = false;
            continue;
        }
        if token == Some("--") {
            normalized.push(arg);
            after_separator = true;
            continue;
        }
        if token.is_some_and(|value| {
            is_grep_long_flag_requiring_next_value(value)
                || is_grep_short_flag_requiring_next_value(value)
        }) {
            normalized.push(arg);
            expects_value = true;
            continue;
        }
        if let Some(bit) = token.and_then(grep_boolean_flag_bit) {
            if seen & bit == 0 {
                normalized.push(arg);
                seen |= bit;
            }
            continue;
        }
        normalized.push(arg);
    }
    normalized
}

fn grep_boolean_flag_bit(token: &str) -> Option<u16> {
    Some(match token {
        "-n" | "--line-number" => 1 << 0,
        "--no-heading" => 1 << 1,
        "-H" | "--with-filename" => 1 << 2,
        "-R" | "--recursive" => 1 << 3,
        "-i" | "--ignore-case" => 1 << 4,
        "-S" | "--smart-case" => 1 << 5,
        "-l" | "--files-with-matches" => 1 << 6,
        "--hidden" => 1 << 7,
        "--no-ignore" => 1 << 8,
        "-a" | "--text" => 1 << 9,
        "-o" | "--only-matching" => 1 << 10,
        "-F" | "--fixed-strings" => 1 << 11,
        "-E" | "--extended-regexp" => 1 << 12,
        "--files" => 1 << 13,
        "--no-compact" => 1 << 14,
        _ => return None,
    })
}

fn preprocess_grep_pattern_args(args: Vec<OsString>) -> Vec<OsString> {
    if args.len() < 3 {
        return args;
    }

    if !matches_subcommand(&args[1], &["grep", "rg"]) {
        return args;
    }

    if args.iter().skip(2).any(|arg| arg == OsStr::new("--")) {
        return args;
    }

    let mut expect_value_for_next = false;
    for index in 2..args.len() {
        let Some(token) = args[index].to_str() else {
            continue;
        };

        if expect_value_for_next {
            expect_value_for_next = false;
            continue;
        }

        if token == "-" || !token.starts_with('-') {
            return args;
        }

        if token.starts_with("--") {
            if is_native_only_search_long_flag(token) {
                return args;
            }
            if is_grep_long_flag_without_value(token) {
                continue;
            }
            if is_grep_long_flag_with_attached_value(token) {
                continue;
            }
            if is_grep_long_flag_requiring_next_value(token) {
                expect_value_for_next = true;
                continue;
            }

            return insert_explicit_regexp_arg(args, index);
        }

        if is_grep_short_flag_cluster_without_value(token) {
            continue;
        }
        if is_grep_short_flag_with_attached_value(token) {
            continue;
        }
        if is_grep_short_flag_requiring_next_value(token) {
            expect_value_for_next = true;
            continue;
        }

        return insert_arg_separator(args, index);
    }

    args
}

fn should_retry_explicit_grep_pattern_preprocess(args: &[OsString]) -> bool {
    if args.len() < 3 || !matches_subcommand(&args[1], &["grep", "rg"]) {
        return false;
    }

    let mut expect_value_for_next = false;
    for arg in args.iter().skip(2) {
        let Some(token) = arg.to_str() else {
            continue;
        };

        if expect_value_for_next {
            expect_value_for_next = false;
            continue;
        }

        if token == "-" || !token.starts_with('-') {
            return false;
        }

        if token.starts_with("--") {
            if is_native_only_search_long_flag(token) {
                return false;
            }
            if is_grep_long_flag_without_value(token)
                || is_grep_long_flag_with_attached_value(token)
            {
                continue;
            }
            if is_grep_long_flag_requiring_next_value(token) {
                expect_value_for_next = true;
                continue;
            }
            return true;
        }

        if is_grep_short_flag_cluster_without_value(token)
            || is_grep_short_flag_with_attached_value(token)
        {
            continue;
        }
        if is_grep_short_flag_requiring_next_value(token) {
            expect_value_for_next = true;
            continue;
        }

        return false;
    }

    false
}

fn matches_subcommand(value: &OsStr, expected: &[&str]) -> bool {
    value
        .to_str()
        .is_some_and(|value| expected.contains(&value))
}

fn passthrough_enabled() -> bool {
    crate::support::insights::unsupported_passthrough_enabled().unwrap_or(false)
}

fn should_passthrough_after_parse_error(kind: ErrorKind, args: &[OsString]) -> bool {
    if matches!(args.get(1), Some(value) if value == OsStr::new("--")) {
        return true;
    }
    let Some(first) = args.get(1).and_then(|arg| arg.to_str()) else {
        return false;
    };
    if first.starts_with('-') {
        return false;
    }
    !is_known_cx_subcommand(first) || kind == ErrorKind::InvalidSubcommand
}

fn passthrough_args(args: &[OsString]) -> Option<Vec<String>> {
    let start = if matches!(args.get(1), Some(value) if value == OsStr::new("--")) {
        2
    } else {
        1
    };
    let values = args
        .get(start..)?
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let program = values.first()?;
    if program.is_empty() || program.starts_with('-') {
        return None;
    }
    Some(values)
}

fn is_known_cx_subcommand(value: &str) -> bool {
    matches!(
        value,
        "git"
            | "diff"
            | "read"
            | "grep"
            | "rg"
            | "ls"
            | "cat"
            | "head"
            | "tail"
            | "sed"
            | "nl"
            | "ps"
            | "pytest"
            | "cargo"
            | "go"
            | "tsc"
            | "node"
            | "sh"
            | "cmake"
            | "ctest"
            | "find"
            | "docker"
            | "kubectl"
            | "report"
            | "insights"
    )
}

fn is_grep_long_flag_without_value(token: &str) -> bool {
    matches!(
        token,
        "--line-number"
            | "--no-heading"
            | "--with-filename"
            | "--recursive"
            | "--ignore-case"
            | "--smart-case"
            | "--files-with-matches"
            | "--hidden"
            | "--no-ignore"
            | "--text"
            | "--only-matching"
            | "--fixed-strings"
            | "--extended-regexp"
            | "--files"
            | "--no-compact"
    )
}

fn is_native_only_search_long_flag(token: &str) -> bool {
    matches!(
        token,
        "--pcre2" | "--auto-hybrid-regex" | "--no-pcre2-unicode" | "--engine" | "--no-filename"
    ) || token.starts_with("--engine=")
}

fn is_grep_long_flag_requiring_next_value(token: &str) -> bool {
    matches!(
        token,
        "--after-context"
            | "--before-context"
            | "--context"
            | "--glob"
            | "--max-count"
            | "--max-results"
            | "--regexp"
            | "--head"
    )
}

fn is_grep_long_flag_with_attached_value(token: &str) -> bool {
    [
        "--after-context=",
        "--before-context=",
        "--context=",
        "--glob=",
        "--max-count=",
        "--max-results=",
        "--regexp=",
        "--head=",
    ]
    .iter()
    .any(|prefix| token.starts_with(prefix))
}

fn is_grep_short_flag_cluster_without_value(token: &str) -> bool {
    token.strip_prefix('-').is_some_and(|flags| {
        !flags.is_empty() && flags.chars().all(is_grep_short_flag_without_value)
    })
}

fn is_grep_short_flag_with_attached_value(token: &str) -> bool {
    let Some(stripped) = token.strip_prefix('-') else {
        return false;
    };

    if stripped.len() < 2 {
        return false;
    }

    let mut chars = stripped.chars();
    matches!(chars.next(), Some('A' | 'B' | 'C' | 'e' | 'g')) && !chars.as_str().is_empty()
}

fn is_grep_short_flag_requiring_next_value(token: &str) -> bool {
    matches!(token, "-A" | "-B" | "-C" | "-e" | "-g")
}

fn is_grep_short_flag_without_value(ch: char) -> bool {
    matches!(
        ch,
        'n' | 'H' | 'R' | 'i' | 'S' | 'l' | 'a' | 'o' | 'F' | 'E'
    )
}

fn insert_arg_separator(mut args: Vec<OsString>, index: usize) -> Vec<OsString> {
    args.insert(index, OsString::from("--"));
    args
}

fn insert_explicit_regexp_arg(mut args: Vec<OsString>, index: usize) -> Vec<OsString> {
    args.insert(index, OsString::from("-e"));
    args
}
