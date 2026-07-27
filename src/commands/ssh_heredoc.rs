#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshHeredocAction {
    None,
    Rewrite {
        forwarded_args: Vec<String>,
        stdin: Vec<u8>,
    },
    Reject {
        message: String,
    },
}

pub fn inspect_ssh_args(args: &[String]) -> SshHeredocAction {
    let Some(command_index) = remote_command_index(args) else {
        return SshHeredocAction::None;
    };
    let remote_command = args[command_index..].join(" ");
    if !remote_command.contains("<<") {
        return SshHeredocAction::None;
    }
    match extract_python_heredoc(&remote_command) {
        Ok(Some((interpreter, script))) => {
            let mut forwarded_args = args[..command_index].to_vec();
            forwarded_args.push(interpreter);
            forwarded_args.push("-".to_string());
            SshHeredocAction::Rewrite {
                forwarded_args,
                stdin: script.into_bytes(),
            }
        }
        Ok(None) => SshHeredocAction::Reject {
            message: remote_heredoc_message(),
        },
        Err(message) => SshHeredocAction::Reject { message },
    }
}

fn remote_command_index(args: &[String]) -> Option<usize> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return (index + 2 < args.len()).then_some(index + 2);
        }
        if arg.starts_with('-') && arg != "-" {
            index += if ssh_option_consumes_next(arg) { 2 } else { 1 };
            continue;
        }
        return (index + 1 < args.len()).then_some(index + 1);
    }
    None
}

fn ssh_option_consumes_next(arg: &str) -> bool {
    if arg.len() != 2 {
        return false;
    }
    matches!(
        arg.as_bytes()[1] as char,
        'B' | 'b'
            | 'c'
            | 'D'
            | 'E'
            | 'e'
            | 'F'
            | 'I'
            | 'i'
            | 'J'
            | 'L'
            | 'l'
            | 'm'
            | 'O'
            | 'o'
            | 'p'
            | 'Q'
            | 'R'
            | 'S'
            | 'W'
            | 'w'
    )
}

fn extract_python_heredoc(command: &str) -> Result<Option<(String, String)>, String> {
    let trimmed = command.trim_start();
    if !looks_like_remote_python_heredoc(trimmed) {
        return Ok(None);
    }
    let Some((first_line, body)) = trimmed.split_once('\n') else {
        return Err(remote_heredoc_message());
    };
    let mut parts = first_line.split_whitespace();
    let Some(interpreter) = parts.next() else {
        return Ok(None);
    };
    if interpreter != "python3" && interpreter != "python" {
        return Ok(None);
    }
    if parts.next() != Some("-") {
        return Ok(None);
    }
    let Some(marker) = parts.next() else {
        return Err(remote_heredoc_message());
    };
    if parts.next().is_some() {
        return Err(remote_heredoc_message());
    }
    let Some(delimiter) = heredoc_delimiter(marker) else {
        return Err(remote_heredoc_message());
    };
    let mut lines = Vec::new();
    let mut found = false;
    for line in body.lines() {
        if !found && line.trim() == delimiter {
            found = true;
            continue;
        }
        if found {
            if !line.trim().is_empty() {
                return Err(remote_heredoc_message());
            }
            continue;
        }
        lines.push(line);
    }
    if !found {
        return Err(remote_heredoc_message());
    }
    let mut script = lines.join("\n");
    script.push('\n');
    Ok(Some((interpreter.to_string(), script)))
}

fn looks_like_remote_python_heredoc(command: &str) -> bool {
    (command.starts_with("python3 ") || command.starts_with("python ")) && command.contains("<<")
}

fn heredoc_delimiter(marker: &str) -> Option<String> {
    let mut value = marker.strip_prefix("<<")?;
    if let Some(stripped) = value.strip_prefix('-') {
        value = stripped;
    }
    value = value.trim();
    if (value.starts_with('\'') && value.ends_with('\''))
        || (value.starts_with('"') && value.ends_with('"'))
    {
        value = &value[1..value.len().saturating_sub(1)];
    }
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

fn remote_heredoc_message() -> String {
    [
        "remote SSH heredoc in a quoted ssh command is unsafe; CX refused to run it before the remote shell could corrupt quotes.",
        "Use stdin forwarding instead:",
        "  cx -- ssh <host> \"python3 -\" <<'PY'",
        "  <python script>",
        "  PY",
        "or for mixed shell/Python setup:",
        "  cx -- ssh <host> \"bash -s\" <<'REMOTE'",
        "  python3 - <<'PY'",
        "  <python script>",
        "  PY",
        "  REMOTE",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{inspect_ssh_args, SshHeredocAction};

    #[test]
    fn extracts_multiline_remote_python_heredoc() {
        let action = inspect_ssh_args(&[
            "build-host-a".to_string(),
            "python3 - <<'PY'\nprint('alpha')\np = '/Users/example/project'\nPY".to_string(),
        ]);
        let SshHeredocAction::Rewrite {
            forwarded_args,
            stdin,
        } = action
        else {
            panic!("expected rewrite");
        };
        assert_eq!(forwarded_args, vec!["build-host-a", "python3", "-"]);
        assert_eq!(
            String::from_utf8(stdin).unwrap(),
            "print('alpha')\np = '/Users/example/project'\n"
        );
    }

    #[test]
    fn rejects_one_line_remote_python_heredoc() {
        let action = inspect_ssh_args(&[
            "build-host-a".to_string(),
            "python3 - <<'PY' print('alpha') PY".to_string(),
        ]);
        let SshHeredocAction::Reject { message } = action else {
            panic!("expected rejection");
        };
        assert!(message.contains("stdin forwarding"));
    }
}
