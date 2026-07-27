pub fn append_hint(stderr: &mut String, combined_output: &str) {
    if let Some(hint) = hint_for_output(combined_output) {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str(hint);
    }
}

fn hint_for_output(output: &str) -> Option<&'static str> {
    if output.contains("zsh:")
        && output.contains("not found")
        && output
            .lines()
            .any(|line| line.contains("==/") || line.contains("==="))
    {
        return Some(
            "hint: zsh may reinterpret unquoted =... marker text; use `printf \"===%s===\\n\" \"$p\"` or quote the marker.",
        );
    }
    if output.contains("zsh:") && output.contains("no matches found: {") {
        return Some(
            "hint: zsh globbed brace text before the script ran; use `cx sh` or `cx -- bash -lc` for Python/heredoc scripts.",
        );
    }
    if output.contains("unsupported file type")
        && output.contains("/dev/fd/")
        && output.contains("cannot hash")
    {
        return Some(
            "hint: git rejected process-substitution /dev/fd paths; use blob specs like `:2:path :3:path` or rerun through `cx -- git diff --no-index` so CX can materialize fd inputs.",
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_zsh_equals_marker_failure() {
        let hint =
            hint_for_output("zsh:1: ==/Volumes/Workspace/project/build=== not found").unwrap();
        assert!(hint.contains("printf"));
    }

    #[test]
    fn detects_zsh_brace_glob_failure() {
        let hint = hint_for_output("zsh:1: no matches found: {b[thread]}").unwrap();
        assert!(hint.contains("cx sh"));
    }

    #[test]
    fn detects_git_fd_failure() {
        let hint = hint_for_output(
            "error: /dev/fd/11: unsupported file type\nfatal: cannot hash /dev/fd/11",
        )
        .unwrap();
        assert!(hint.contains("blob specs"));
    }
}
