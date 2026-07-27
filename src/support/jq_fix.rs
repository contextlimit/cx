const PRECEDENCE_ERROR: &str = "Cannot iterate over number";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JqRepair {
    pub args: Vec<String>,
    pub original_filter: String,
    pub repaired_filter: String,
}

pub fn repaired_args_for_precedence_failure(
    args: &[String],
    combined_output: &str,
) -> Option<JqRepair> {
    if !looks_like_precedence_failure(combined_output) {
        return None;
    }
    let filter_index = filter_arg_index(args)?;
    let original_filter = args.get(filter_index)?;
    let repaired_filter = repair_filter_precedence(original_filter)?;
    let mut repaired_args = args.to_vec();
    repaired_args[filter_index] = repaired_filter.clone();
    Some(JqRepair {
        args: repaired_args,
        original_filter: original_filter.clone(),
        repaired_filter,
    })
}

pub fn repair_hint(repair: &JqRepair) -> String {
    format!(
        "hint: jq comma branches after a pipe can need explicit parentheses; CX repaired the filter as:\n  {}",
        repair.repaired_filter
    )
}

pub fn repaired_filter_hint(filter: &str, combined_output: &str) -> Option<String> {
    let args = vec![filter.to_string()];
    repaired_args_for_precedence_failure(&args, combined_output).map(|repair| repair_hint(&repair))
}

fn looks_like_precedence_failure(output: &str) -> bool {
    output.contains("jq:") && output.contains(PRECEDENCE_ERROR)
}

fn repair_filter_precedence(filter: &str) -> Option<String> {
    let parts = split_top_level(filter, ',')?;
    if parts.len() < 2 || !parts.iter().any(|part| part.contains('|')) {
        return None;
    }
    let repaired = parts
        .iter()
        .map(|part| wrap_filter_branch(part.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    (repaired != filter.trim()).then_some(repaired)
}

fn wrap_filter_branch(branch: &str) -> String {
    if outer_parens_wrap_all(branch) {
        branch.to_string()
    } else {
        format!("({branch})")
    }
}

fn outer_parens_wrap_all(value: &str) -> bool {
    if !(value.starts_with('(') && value.ends_with(')')) {
        return false;
    }
    let mut depth = 0i32;
    for (index, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && index + ch.len_utf8() < value.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn split_top_level(value: &str, delimiter: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            _ if ch == delimiter && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
        if paren_depth < 0 || bracket_depth < 0 || brace_depth < 0 {
            return None;
        }
    }
    if in_string || paren_depth != 0 || bracket_depth != 0 || brace_depth != 0 {
        return None;
    }
    parts.push(value[start..].trim());
    Some(parts)
}

pub fn filter_arg_index(args: &[String]) -> Option<usize> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return (index + 1 < args.len()).then_some(index + 1);
        }
        if arg == "-f" || arg == "--from-file" || arg.starts_with("--from-file=") {
            return None;
        }
        if jq_option_consumes_values(arg) {
            index = index.checked_add(3)?;
            continue;
        }
        if jq_option_consumes_one_value(arg) {
            index = index.checked_add(2)?;
            continue;
        }
        if jq_flag_without_value(arg) || jq_short_flag_cluster(arg) {
            index += 1;
            continue;
        }
        if arg.starts_with('-') && arg != "-" {
            return None;
        }
        return Some(index);
    }
    None
}

fn jq_option_consumes_values(arg: &str) -> bool {
    matches!(arg, "--arg" | "--argjson" | "--slurpfile" | "--rawfile")
}

fn jq_option_consumes_one_value(arg: &str) -> bool {
    matches!(arg, "-L" | "--library-path")
}

fn jq_flag_without_value(arg: &str) -> bool {
    matches!(
        arg,
        "-r" | "--raw-output"
            | "-c"
            | "--compact-output"
            | "-M"
            | "--monochrome-output"
            | "-C"
            | "--color-output"
            | "-S"
            | "--sort-keys"
            | "-s"
            | "--slurp"
            | "-n"
            | "--null-input"
            | "-e"
            | "--exit-status"
            | "-R"
            | "--raw-input"
            | "-j"
            | "--join-output"
            | "--seq"
            | "--stream"
    )
}

fn jq_short_flag_cluster(arg: &str) -> bool {
    arg.strip_prefix('-').is_some_and(|flags| {
        flags.len() > 1
            && flags.chars().all(|ch| {
                matches!(
                    ch,
                    'r' | 'c' | 'M' | 'C' | 'S' | 's' | 'n' | 'e' | 'R' | 'j'
                )
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_jq_top_level_comma_precedence() {
        let filter = r#"[.[] | select(.status=="completed")] | length, [.[] | select(.status=="in_progress")] | map(.id) | join(",")"#;
        let repair = repaired_args_for_precedence_failure(
            &[
                "-r".to_string(),
                filter.to_string(),
                "ledger.json".to_string(),
            ],
            "jq: error: Cannot iterate over number (10)",
        )
        .unwrap();

        assert_eq!(
            repair.repaired_filter,
            r#"([.[] | select(.status=="completed")] | length), ([.[] | select(.status=="in_progress")] | map(.id) | join(","))"#
        );
        assert_eq!(repair.args[1], repair.repaired_filter);
    }

    #[test]
    fn ignores_non_precedence_failures() {
        assert!(repaired_args_for_precedence_failure(
            &[".[]".to_string()],
            "jq: parse error: Invalid numeric literal"
        )
        .is_none());
    }
}
