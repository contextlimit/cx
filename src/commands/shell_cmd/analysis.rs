use crate::support::document_formats::{is_exact_read_path, is_human_source_path};

const MAX_EXACT_BOUNDED_LINES: usize = 1_000;
const MAX_EXACT_DD_BYTES: usize = 1024 * 1024;

pub(super) fn should_preserve_exact_stdout(args: &[String]) -> bool {
    shell_command_arg(args).is_some_and(|script| {
        is_exact_source_anchor_script(script)
            || is_exact_path_read_script(script)
            || is_bounded_shell_output_script(script)
    })
}

fn is_exact_source_anchor_script(script: &str) -> bool {
    let statements = split_shell_statements(script);
    !statements.is_empty()
        && statements
            .iter()
            .all(|statement| is_numbered_sed_range_pipeline(statement))
}

fn is_exact_path_read_script(script: &str) -> bool {
    let statements = split_shell_statements(script);
    !statements.is_empty()
        && statements
            .iter()
            .all(|statement| is_exact_path_read_statement(statement))
}

fn is_exact_path_read_statement(statement: &str) -> bool {
    if let Some(path) = numbered_sed_range_pipeline_path(statement) {
        return is_exact_read_path(&path);
    }
    let Some(words) = split_shell_words(statement) else {
        return false;
    };
    match words.first().map(String::as_str) {
        Some("cat") => {
            simple_command_path(&words[1..], &["-n", "--number"]).is_some_and(is_exact_read_path)
        }
        Some("head" | "tail") => head_tail_path(&words[1..]).is_some_and(is_exact_read_path),
        Some("sed") => sed_read_path(&words[1..])
            .is_some_and(|path| is_exact_read_path(path) || is_human_source_path(path)),
        Some("nl") => nl_read_path(&words[1..]).is_some_and(is_exact_read_path),
        Some("dd") => bounded_dd_read_path(&words[1..]).is_some_and(is_exact_read_path),
        _ => false,
    }
}

fn bounded_dd_read_path(words: &[String]) -> Option<&str> {
    let mut path = None;
    let mut block_size = 512usize;
    let mut count = None;
    for word in words {
        if let Some(value) = word.strip_prefix("if=") {
            if value.is_empty() || path.replace(value).is_some() {
                return None;
            }
        } else if word.starts_with("of=") {
            return None;
        } else if let Some(value) = word.strip_prefix("bs=") {
            block_size = positive_decimal(value)?;
        } else if let Some(value) = word.strip_prefix("count=") {
            count = Some(positive_decimal(value)?);
        } else if let Some(value) = word.strip_prefix("skip=") {
            nonnegative_decimal(value)?;
        } else if matches!(
            word.as_str(),
            "status=none" | "status=noxfer" | "2>/dev/null"
        ) {
        } else {
            return None;
        }
    }
    let estimated_bytes = block_size.checked_mul(count?)?;
    (estimated_bytes <= MAX_EXACT_DD_BYTES)
        .then_some(path)
        .flatten()
}

fn is_bounded_shell_output_script(script: &str) -> bool {
    let statements = split_shell_statements(script);
    !statements.is_empty()
        && statements
            .iter()
            .all(|statement| bounded_shell_statement_lines(statement).is_some())
}

fn bounded_shell_statement_lines(statement: &str) -> Option<usize> {
    let commands = split_outside_quotes(statement, b"|")
        .into_iter()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .collect::<Vec<_>>();
    let command = commands.last()?;
    let words = split_shell_words(command)?;
    let lines = bounded_words_line_count(&words)?;
    (lines <= MAX_EXACT_BOUNDED_LINES).then_some(lines)
}

fn bounded_words_line_count(words: &[String]) -> Option<usize> {
    match words.first().map(String::as_str) {
        Some("head") => head_line_count(&words[1..]),
        Some("tail") => tail_line_count(&words[1..]),
        Some("sed") => sed_line_count(&words[1..]),
        _ => None,
    }
}

pub(super) fn split_shell_statements(script: &str) -> Vec<&str> {
    split_outside_quotes(script, b";\n")
        .into_iter()
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .collect()
}

fn is_numbered_sed_range_pipeline(statement: &str) -> bool {
    numbered_sed_range_pipeline_path(statement).is_some()
}

fn numbered_sed_range_pipeline_path(statement: &str) -> Option<String> {
    let pipe_index = find_delimiter_outside_quotes(statement, b'|')?;
    let left = statement[..pipe_index].trim();
    let right = statement[pipe_index + 1..].trim();
    (find_delimiter_outside_quotes(right, b'|').is_none() && is_sed_range_filter(right))
        .then(|| numbered_file_command_path(left))
        .flatten()
}

fn numbered_file_command_path(command: &str) -> Option<String> {
    let words = split_shell_words(command)?;
    if words.first().map(String::as_str) != Some("nl") {
        return None;
    }
    let mut has_all_lines_mode = false;
    let mut path = None;
    let mut index = 1usize;
    while index < words.len() {
        match words[index].as_str() {
            "-ba" => has_all_lines_mode = true,
            "-b" if words.get(index + 1).map(String::as_str) == Some("a") => {
                has_all_lines_mode = true;
                index += 1;
            }
            "--" => {
                path = single_remaining_word(&words, index + 1);
                break;
            }
            value if value.starts_with('-') => {}
            value => set_single_word(&mut path, value)?,
        }
        index += 1;
    }
    has_all_lines_mode
        .then_some(path)
        .flatten()
        .map(str::to_string)
}

fn nl_read_path(words: &[String]) -> Option<&str> {
    let mut path = None;
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "-ba" => index += 1,
            "-b" if words.get(index + 1).map(String::as_str) == Some("a") => index += 2,
            "--" => return single_remaining_word(words, index + 1),
            value if value.starts_with('-') => return None,
            value => {
                set_single_word(&mut path, value)?;
                index += 1;
            }
        }
    }
    path
}

fn simple_command_path<'a>(words: &'a [String], accepted_flags: &[&str]) -> Option<&'a str> {
    let mut path = None;
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "--" => return single_remaining_word(words, index + 1),
            value if accepted_flags.contains(&value) => {}
            value if value.starts_with('-') => return None,
            value => set_single_word(&mut path, value)?,
        }
        index += 1;
    }
    path
}

fn head_tail_path(words: &[String]) -> Option<&str> {
    let mut path = None;
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "--" => return single_remaining_word(words, index + 1),
            "-n" | "--lines" => index += 1,
            value if value.starts_with("--lines=") || short_numeric_count(value) => {}
            value if value.starts_with('-') => return None,
            value => set_single_word(&mut path, value)?,
        }
        index += 1;
    }
    path
}

fn head_line_count(words: &[String]) -> Option<usize> {
    let mut line_count = 10usize;
    let mut operands = 0usize;
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "-n" | "--lines" => {
                index += 1;
                line_count = parse_head_count(words.get(index)?.as_str())?;
            }
            value if value.starts_with("--lines=") => {
                line_count = parse_head_count(value.strip_prefix("--lines=")?)?;
            }
            value if value.starts_with("-n") && value.len() > 2 => {
                line_count = parse_head_count(&value[2..])?;
            }
            value if short_numeric_count(value) => line_count = parse_short_count(value)?,
            "--" => {
                operands = operands.checked_add(words.len().saturating_sub(index + 1))?;
                break;
            }
            value if value.starts_with('-') => return None,
            _ => operands = operands.checked_add(1)?,
        }
        index += 1;
    }
    (operands <= 1).then_some(line_count)
}

fn tail_line_count(words: &[String]) -> Option<usize> {
    let mut line_count = 10usize;
    let mut operands = 0usize;
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "-n" | "--lines" => {
                index += 1;
                line_count = parse_tail_bounded_count(words.get(index)?.as_str())?;
            }
            value if value.starts_with("--lines=") => {
                line_count = parse_tail_bounded_count(value.strip_prefix("--lines=")?)?;
            }
            value if value.starts_with("-n") && value.len() > 2 => {
                line_count = parse_tail_bounded_count(&value[2..])?;
            }
            value if short_numeric_count(value) => line_count = parse_short_count(value)?,
            "--" => {
                operands = operands.checked_add(words.len().saturating_sub(index + 1))?;
                break;
            }
            value if value.starts_with('-') => return None,
            _ => operands = operands.checked_add(1)?,
        }
        index += 1;
    }
    (operands <= 1).then_some(line_count)
}

fn sed_line_count(words: &[String]) -> Option<usize> {
    let mut quiet = false;
    let mut script = None;
    let mut operands = 0usize;
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "-n" | "--quiet" | "--silent" => quiet = true,
            "--" => {
                operands = operands.checked_add(words.len().saturating_sub(index + 1))?;
                break;
            }
            value if value.starts_with('-') => return None,
            value if script.is_none() => script = Some(value),
            _ => operands = operands.checked_add(1)?,
        }
        index += 1;
    }
    (quiet && operands <= 1).then(|| sed_print_range_line_count(script?))?
}

fn sed_read_path(words: &[String]) -> Option<&str> {
    let mut quiet = false;
    let mut script = None;
    let mut path = None;
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "-n" | "--quiet" | "--silent" => quiet = true,
            "--" => {
                path = single_remaining_word(words, index + 1);
                break;
            }
            value if value.starts_with('-') => return None,
            value if script.is_none() => script = Some(value),
            value => set_single_word(&mut path, value)?,
        }
        index += 1;
    }
    (quiet && script.is_some_and(is_sed_print_range))
        .then_some(path)
        .flatten()
}

fn single_remaining_word(words: &[String], start: usize) -> Option<&str> {
    (words.len().checked_sub(start)? == 1).then(|| words[start].as_str())
}

fn set_single_word<'a>(path: &mut Option<&'a str>, value: &'a str) -> Option<()> {
    if path.replace(value).is_some() {
        return None;
    }
    Some(())
}

fn short_numeric_count(value: &str) -> bool {
    let Some(count) = value.strip_prefix('-') else {
        return false;
    };
    !count.is_empty() && count.chars().all(|ch| ch.is_ascii_digit())
}

fn parse_head_count(value: &str) -> Option<usize> {
    positive_decimal(value)
}

fn parse_tail_bounded_count(value: &str) -> Option<usize> {
    if value.starts_with('+') {
        return None;
    }
    positive_decimal(value)
}

fn parse_short_count(value: &str) -> Option<usize> {
    positive_decimal(value.strip_prefix('-')?)
}

fn is_sed_range_filter(command: &str) -> bool {
    let Some(words) = split_shell_words(command) else {
        return false;
    };
    words.len() == 3 && words[0] == "sed" && words[1] == "-n" && is_sed_print_range(&words[2])
}

fn is_sed_print_range(script: &str) -> bool {
    let ranges = split_outside_quotes(script, b";")
        .into_iter()
        .map(str::trim)
        .filter(|range| !range.is_empty())
        .collect::<Vec<_>>();
    !ranges.is_empty() && ranges.into_iter().all(is_single_sed_print_range)
}

fn sed_print_range_line_count(script: &str) -> Option<usize> {
    let ranges = split_outside_quotes(script, b";")
        .into_iter()
        .map(str::trim)
        .filter(|range| !range.is_empty())
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return None;
    }
    ranges.into_iter().try_fold(0usize, |total, range| {
        total.checked_add(sed_single_range_line_count(range)?)
    })
}

fn is_single_sed_print_range(script: &str) -> bool {
    let Some(body) = script.strip_suffix('p') else {
        return false;
    };
    if body.is_empty() {
        return false;
    }
    if let Some((start, end)) = body.split_once(',') {
        return is_positive_decimal(start.trim())
            && (is_positive_decimal(end.trim()) || end.trim() == "$");
    }
    is_positive_decimal(body.trim())
}

fn sed_single_range_line_count(script: &str) -> Option<usize> {
    let body = script.strip_suffix('p')?;
    if body.is_empty() {
        return None;
    }
    if let Some((start, end)) = body.split_once(',') {
        let start = positive_decimal(start.trim())?;
        let end = positive_decimal(end.trim())?;
        return (end >= start).then_some(end - start + 1);
    }
    positive_decimal(body.trim()).map(|_| 1)
}

fn is_positive_decimal(value: &str) -> bool {
    positive_decimal(value).is_some()
}

fn positive_decimal(value: &str) -> Option<usize> {
    (!value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
        .then(|| value.parse::<usize>().ok())?
        .filter(|value| *value > 0)
}

fn nonnegative_decimal(value: &str) -> Option<usize> {
    (!value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
        .then(|| value.parse::<usize>().ok())?
}

pub(super) fn split_shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ch if ch.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if in_single || in_double {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

fn split_outside_quotes<'a>(value: &'a str, delimiters: &[u8]) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    let bytes = value.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_double => index += 2,
            b'\'' if !in_double => {
                in_single = !in_single;
                index += 1;
            }
            b'"' if !in_single => {
                in_double = !in_double;
                index += 1;
            }
            byte if !in_single && !in_double && delimiters.contains(&byte) => {
                parts.push(&value[start..index]);
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    parts.push(&value[start..]);
    parts
}

fn find_delimiter_outside_quotes(value: &str, delimiter: u8) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_double => index += 2,
            b'\'' if !in_double => {
                in_single = !in_single;
                index += 1;
            }
            b'"' if !in_single => {
                in_double = !in_double;
                index += 1;
            }
            byte if !in_single && !in_double && byte == delimiter => return Some(index),
            _ => index += 1,
        }
    }
    None
}

pub(super) fn literal_newline_script_error(args: &[String]) -> Option<String> {
    let script = shell_command_arg(args)?;
    if script.contains('\n') || !script.contains("\\n") {
        return None;
    }
    if has_heredoc_operator_before_escaped_newline(script) {
        return Some(
            "shell heredoc contains literal \\n escapes instead of real newlines; CX did not run it because bash would not receive the intended heredoc.\n\
Use a real local heredoc shape instead:\n\
  cx sh <<'BASH'\n\
  python3 - <<'PY'\n\
  ...\n\
  PY\n\
  BASH\n\
Or pass a real multiline script to `cx -- bash -lc $'...'`."
                .to_string(),
        );
    }
    if !has_serialized_multiline_separators(script) {
        return None;
    }
    Some(
        "shell command contains serialized \\n separators instead of real newlines; CX did not run it because the invoking shell may already have expanded variables or command substitutions before bash received the script.\n\
Use a real local heredoc so the script bytes remain intact:\n\
  cx sh <<'BASH'\n\
  for item in ...; do\n\
    ...\n\
  done\n\
  BASH\n\
Do not put a multiline script containing `$variables` or `$(commands)` in a double-quoted `bash -lc` argument."
            .to_string(),
    )
}

fn has_serialized_multiline_separators(script: &str) -> bool {
    let separators = escaped_newline_separators(script);
    if separators.len() < 2 {
        return false;
    }
    let trimmed = script.trim();
    trimmed.starts_with("\\n")
        || trimmed.ends_with("\\n")
        || separators
            .into_iter()
            .any(|index| starts_with_shell_block_keyword(&script[index + 2..]))
}

fn escaped_newline_separators(script: &str) -> Vec<usize> {
    let bytes = script.as_bytes();
    let mut separators = Vec::new();
    let mut index = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if !in_single && !in_double && bytes.get(index + 1).copied() == Some(b'n') => {
                separators.push(index);
                index += 2;
            }
            b'\\' if !in_single => index += 2,
            b'\'' if !in_double => {
                in_single = !in_single;
                index += 1;
            }
            b'"' if !in_single => {
                in_double = !in_double;
                index += 1;
            }
            _ => index += 1,
        }
    }
    separators
}

fn starts_with_shell_block_keyword(value: &str) -> bool {
    let value = value.trim_start();
    [
        "for", "while", "until", "if", "then", "else", "elif", "fi", "do", "done", "case", "esac",
        "function",
    ]
    .iter()
    .any(|keyword| starts_with_shell_word(value, keyword))
}

fn starts_with_shell_word(value: &str, word: &str) -> bool {
    let Some(rest) = value.strip_prefix(word) else {
        return false;
    };
    rest.is_empty()
        || rest
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace() || matches!(ch, ';' | '\\'))
}

pub(super) fn shell_command_arg(args: &[String]) -> Option<&str> {
    args.iter()
        .enumerate()
        .find_map(|(index, arg)| is_bash_command_option(arg).then(|| args.get(index + 1))?)
        .map(String::as_str)
}

fn is_bash_command_option(arg: &str) -> bool {
    arg == "-c"
        || (arg.starts_with('-')
            && !arg.starts_with("--")
            && arg
                .strip_prefix('-')
                .is_some_and(|flags| flags.contains('c')))
}

fn has_heredoc_operator_before_escaped_newline(script: &str) -> bool {
    let bytes = script.as_bytes();
    let mut index = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    while index + 1 < bytes.len() {
        match bytes[index] {
            b'\\' if in_double => index += 2,
            b'\'' if !in_double => {
                in_single = !in_single;
                index += 1;
            }
            b'"' if !in_single => {
                in_double = !in_double;
                index += 1;
            }
            b'<' if !in_single && !in_double && bytes[index + 1] == b'<' => {
                if heredoc_marker_is_followed_by_escaped_newline(&script[index + 2..]) {
                    return true;
                }
                index += 2;
            }
            _ => index += 1,
        }
    }
    false
}

fn heredoc_marker_is_followed_by_escaped_newline(after_operator: &str) -> bool {
    let trimmed = after_operator
        .strip_prefix('-')
        .unwrap_or(after_operator)
        .trim_start();
    let Some(rest) = trimmed.strip_prefix('\'') else {
        return unquoted_heredoc_marker_is_followed_by_escaped_newline(trimmed);
    };
    let Some((_, after_quote)) = rest.split_once('\'') else {
        return false;
    };
    after_quote.starts_with("\\n")
}

fn unquoted_heredoc_marker_is_followed_by_escaped_newline(trimmed: &str) -> bool {
    let marker_len = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    marker_len > 0 && trimmed[marker_len..].starts_with("\\n")
}
