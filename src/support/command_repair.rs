use std::fs;
use std::path::{Path, PathBuf};

use crate::support::jq_fix;

const SQLITE_BUSY_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectCommandRepair {
    pub rule_id: &'static str,
    pub args: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAdvice {
    pub rule_id: &'static str,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectCommandRewrite {
    pub rule_id: &'static str,
    pub args: Vec<String>,
}

pub fn direct_passthrough_rewrite(
    program: &str,
    args: &[String],
    insights_database: &Path,
) -> Option<DirectCommandRewrite> {
    if executable_name(program) != "sqlite3" || has_sqlite_timeout(args) {
        return None;
    }
    let database_index = args
        .iter()
        .position(|arg| same_existing_path(arg, insights_database))?;
    let insertion_index = args
        .iter()
        .take(database_index)
        .position(|arg| arg == "--")
        .unwrap_or(database_index);
    let mut rewritten = args.to_vec();
    rewritten.splice(
        insertion_index..insertion_index,
        [
            "-cmd".to_string(),
            format!(".timeout {SQLITE_BUSY_TIMEOUT_MS}"),
        ],
    );
    Some(DirectCommandRewrite {
        rule_id: "sqlite3.cx_database_busy_timeout",
        args: rewritten,
    })
}

pub fn direct_passthrough_repair(
    program: &str,
    args: &[String],
    exit_code: i32,
    combined_output: &str,
) -> Option<DirectCommandRepair> {
    if exit_code == 0 {
        return None;
    }
    match program {
        "jq" => jq_fix::repaired_args_for_precedence_failure(args, combined_output).map(|repair| {
            let note = jq_fix::repair_hint(&repair);
            DirectCommandRepair {
                rule_id: "jq.comma_pipe_precedence",
                args: repair.args,
                note,
            }
        }),
        _ => None,
    }
}

fn executable_name(program: &str) -> &str {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
}

fn has_sqlite_timeout(args: &[String]) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == "-cmd" && pair[1].trim_start().starts_with(".timeout"))
}

fn same_existing_path(candidate: &str, expected: &Path) -> bool {
    canonical_or_absolute(Path::new(candidate))
        .zip(canonical_or_absolute(expected))
        .is_some_and(|(candidate, expected)| candidate == expected)
}

fn canonical_or_absolute(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Some(canonical);
    }
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    std::env::current_dir().ok().map(|cwd| cwd.join(path))
}

pub fn shell_command_advice(
    program: &str,
    args: &[String],
    exit_code: i32,
    combined_output: &str,
) -> Option<CommandAdvice> {
    if exit_code == 0 {
        return None;
    }
    match program {
        "jq" => jq_shell_precedence_advice(args, combined_output),
        _ => None,
    }
}

pub fn node_runtime_advice(
    _args: &[String],
    exit_code: i32,
    combined_output: &str,
) -> Option<CommandAdvice> {
    if exit_code == 0 || !is_node_ambiguous_module_syntax(combined_output) {
        return None;
    }
    Some(CommandAdvice {
        rule_id: "node.ambiguous_module_syntax",
        note: "CX detected Node's ambiguous stdin module-format error: the script mixes `require()` with top-level `await`. For CommonJS heredocs, wrap the awaited code in an async function. For ES module heredocs, use `cx -- node --input-type=module` and replace `require()` with `import`.".to_string(),
    })
}

pub fn append_note(stderr: &mut String, note: &str) {
    if !stderr.is_empty() && !stderr.ends_with('\n') {
        stderr.push('\n');
    }
    stderr.push_str(note);
}

fn is_node_ambiguous_module_syntax(output: &str) -> bool {
    output.contains("ERR_AMBIGUOUS_MODULE_SYNTAX")
        || output.contains("Cannot determine intended module format")
            && output.contains("require")
            && output.contains("top-level await")
}

fn jq_shell_precedence_advice(args: &[String], combined_output: &str) -> Option<CommandAdvice> {
    let filter = args
        .get(jq_fix::filter_arg_index(args)?)
        .map(String::as_str)?;
    jq_fix::repaired_filter_hint(filter, combined_output).map(|note| CommandAdvice {
        rule_id: "jq.comma_pipe_precedence",
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const BAD_JQ_FILTER: &str = r#"[.[] | select(.status=="completed")] | length, [.[] | select(.status=="in_progress")] | map(.id) | join(",")"#;
    const REPAIRED_JQ_FILTER: &str = r#"([.[] | select(.status=="completed")] | length), ([.[] | select(.status=="in_progress")] | map(.id) | join(","))"#;
    const JQ_ERROR: &str = "jq: error: Cannot iterate over number (10)";

    #[test]
    fn direct_repair_returns_retry_args_for_read_only_jq_failure() {
        let repair = direct_passthrough_repair(
            "jq",
            &[
                "-r".to_string(),
                BAD_JQ_FILTER.to_string(),
                "ledger.json".to_string(),
            ],
            5,
            JQ_ERROR,
        )
        .unwrap();

        assert_eq!(repair.rule_id, "jq.comma_pipe_precedence");
        assert_eq!(repair.args[1], REPAIRED_JQ_FILTER);
        assert!(repair.note.contains("CX repaired the filter"));
    }

    #[test]
    fn direct_repair_ignores_success_and_unknown_programs() {
        assert!(direct_passthrough_repair("jq", &[".".to_string()], 0, JQ_ERROR).is_none());
        assert!(
            direct_passthrough_repair("python3", &["script.py".to_string()], 1, JQ_ERROR).is_none()
        );
    }

    #[test]
    fn shell_advice_returns_note_without_retry_args() {
        let advice = shell_command_advice(
            "jq",
            &[
                "-r".to_string(),
                BAD_JQ_FILTER.to_string(),
                "ledger.json".to_string(),
            ],
            5,
            JQ_ERROR,
        )
        .unwrap();

        assert_eq!(advice.rule_id, "jq.comma_pipe_precedence");
        assert!(advice.note.contains(REPAIRED_JQ_FILTER));
    }

    #[test]
    fn node_advice_explains_ambiguous_stdin_module_format() {
        let advice = node_runtime_advice(
            &[],
            1,
            "ReferenceError: Cannot determine intended module format because both 'require' and top-level await are present\ncode: 'ERR_AMBIGUOUS_MODULE_SYNTAX'",
        )
        .unwrap();

        assert_eq!(advice.rule_id, "node.ambiguous_module_syntax");
        assert!(advice.note.contains("require()"));
        assert!(advice.note.contains("top-level `await`"));
        assert!(advice.note.contains("--input-type=module"));
    }

    #[test]
    fn sqlite_rewrite_adds_busy_timeout_for_the_cx_database() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("db.sqlite");
        fs::write(&database, "").unwrap();
        let rewrite = direct_passthrough_rewrite(
            "sqlite3",
            &[
                "-readonly".to_string(),
                database.display().to_string(),
                "SELECT 1".to_string(),
            ],
            &database,
        )
        .unwrap();

        assert_eq!(rewrite.rule_id, "sqlite3.cx_database_busy_timeout");
        assert_eq!(
            rewrite.args,
            vec![
                "-readonly",
                "-cmd",
                ".timeout 5000",
                database.to_str().unwrap(),
                "SELECT 1"
            ]
        );
    }

    #[test]
    fn sqlite_rewrite_preserves_explicit_timeout_and_other_databases() {
        let temp = tempfile::tempdir().unwrap();
        let insights = temp.path().join("insights.sqlite");
        let other = temp.path().join("other.sqlite");
        fs::write(&insights, "").unwrap();
        fs::write(&other, "").unwrap();

        assert!(direct_passthrough_rewrite(
            "sqlite3",
            &[
                "-cmd".to_string(),
                ".timeout 250".to_string(),
                insights.display().to_string(),
            ],
            &insights,
        )
        .is_none());
        assert!(direct_passthrough_rewrite(
            "sqlite3",
            &[other.display().to_string(), "SELECT 1".to_string()],
            &insights,
        )
        .is_none());
    }
}
