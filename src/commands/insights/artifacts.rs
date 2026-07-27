use crate::support::insights::CommandLevel;

pub(crate) fn failure_artifact_tool_name(command: &str, level: CommandLevel) -> String {
    let root = command
        .split_whitespace()
        .next()
        .filter(|tool| !tool.is_empty())
        .unwrap_or(command);
    if root == "passthrough" {
        return "passthrough".to_string();
    }

    let tool = match level {
        CommandLevel::Root => root_artifact_tool(root),
        CommandLevel::Command => command_artifact_tool(command, root),
    };
    tool.to_string()
}

fn root_artifact_tool(root: &str) -> &str {
    match root {
        "git" | "diff" => "git",
        "grep" | "rg" => "grep",
        "bash" | "sh" => "sh",
        "cargo" | "cmake" | "ctest" | "docker" | "go" | "kubectl" | "ls" | "node" | "ps"
        | "pytest" | "tsc" => root,
        _ => "passthrough",
    }
}

fn command_artifact_tool<'a>(command: &str, root: &'a str) -> &'a str {
    if command == "diff" || command.starts_with("git ") {
        "git"
    } else if matches!(root, "grep" | "rg") {
        "grep"
    } else if matches!(command, "bash" | "sh") {
        "sh"
    } else if command == "cmake build" {
        "cmake"
    } else if matches!(
        root,
        "cargo" | "ctest" | "docker" | "go" | "kubectl" | "ls" | "node" | "ps" | "pytest" | "tsc"
    ) {
        root
    } else {
        "passthrough"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_command_families_to_their_execution_artifact_directory() {
        assert_eq!(
            failure_artifact_tool_name("rg extended", CommandLevel::Command),
            "grep"
        );
        assert_eq!(
            failure_artifact_tool_name("git log", CommandLevel::Command),
            "git"
        );
        assert_eq!(
            failure_artifact_tool_name("cmake configure", CommandLevel::Command),
            "passthrough"
        );
        assert_eq!(
            failure_artifact_tool_name("passthrough ssh", CommandLevel::Command),
            "passthrough"
        );
    }

    #[test]
    fn maps_unsupported_and_redacted_roots_to_passthrough_artifacts() {
        for root in ["ssh", "wc", "unknown", "clang-format", "custom-test-binary"] {
            assert_eq!(
                failure_artifact_tool_name(root, CommandLevel::Root),
                "passthrough"
            );
        }
        assert_eq!(failure_artifact_tool_name("rg", CommandLevel::Root), "grep");
        assert_eq!(
            failure_artifact_tool_name("diff", CommandLevel::Root),
            "git"
        );
    }
}
