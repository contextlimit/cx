use crate::support::redaction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandIdentity {
    pub argv: Vec<String>,
    pub root: String,
    pub family: String,
}

impl CommandIdentity {
    pub fn classify(args: &[String]) -> Self {
        let argv = normalized_argv(args);
        let root = argv
            .first()
            .map(|program| canonical_program_name(program))
            .unwrap_or_else(|| "unknown".to_string());
        let family = classify_family(&root, argv.get(1..).unwrap_or_default());
        Self { argv, root, family }
    }
}

fn normalized_argv(args: &[String]) -> Vec<String> {
    let mut argv = if let [command] = args {
        split_report_command(command).unwrap_or_else(|| args.to_vec())
    } else {
        args.to_vec()
    };
    let mut start = 0usize;
    loop {
        while argv.get(start).is_some_and(|arg| arg == "--") {
            start += 1;
        }
        if argv.get(start).is_some_and(|arg| is_cx_program(arg)) {
            start += 1;
            continue;
        }
        break;
    }
    if start > 0 {
        argv.drain(..start);
    }
    argv
}

fn split_report_command(command: &str) -> Option<Vec<String>> {
    let words = split_shell_words(command)?;
    (words.len() > 1).then_some(words)
}

// This parser is reporting-only: it never expands variables, substitutions, or operators.
fn split_shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars();
    let mut in_single = false;
    let mut in_double = false;
    let mut word_started = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\\' if !in_single => {
                current.push(chars.next()?);
                word_started = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                word_started = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                word_started = true;
            }
            ch if ch.is_whitespace() && !in_single && !in_double => {
                if word_started {
                    words.push(std::mem::take(&mut current));
                    word_started = false;
                }
            }
            _ => {
                current.push(ch);
                word_started = true;
            }
        }
    }

    if in_single || in_double {
        return None;
    }
    if word_started {
        words.push(current);
    }
    Some(words)
}

fn is_cx_program(program: &str) -> bool {
    program_name(program) == "cx"
}

fn program_name(program: &str) -> &str {
    program
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(program)
}

fn canonical_program_name(program: &str) -> String {
    redaction::telemetry_program_name(program).unwrap_or_else(|| "unknown".to_string())
}

fn classify_family(root: &str, args: &[String]) -> String {
    match root {
        "grep" | "rg" => search_family(root, args),
        "git" => git_family(args),
        "cargo" => recognized_subcommand_family(
            "cargo",
            args,
            &["test"],
            &["--manifest-path", "--config", "--target-dir"],
            true,
        ),
        "go" => recognized_subcommand_family("go", args, &["test"], &[], false),
        "cmake" => cmake_family(args),
        "docker" => recognized_subcommand_family(
            "docker",
            args,
            &["ps", "logs"],
            &["--context", "--host", "-H", "--config", "--log-level"],
            false,
        ),
        "kubectl" => recognized_subcommand_family(
            "kubectl",
            args,
            &["logs"],
            &[
                "--context",
                "--namespace",
                "-n",
                "--cluster",
                "--server",
                "--user",
                "--kubeconfig",
            ],
            false,
        ),
        "node" => node_family(args),
        "npm" => npm_family(args),
        "npx" => npx_family(args),
        "dotnet" => dotnet_family(args),
        "clang-format" => clang_format_family(args),
        "bash" | "sh" => "sh".to_string(),
        "sed" => "sed range".to_string(),
        "diff" | "read" | "ls" | "cat" | "head" | "tail" | "nl" | "ps" | "pytest" | "tsc"
        | "ctest" | "find" | "report" | "insights" => root.to_string(),
        "unknown" => "unknown".to_string(),
        _ => passthrough_family(root),
    }
}

fn search_family(root: &str, args: &[String]) -> String {
    let mut files = false;
    let mut fixed = false;
    let mut extended = root == "rg";
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--files" => files = true,
            "--fixed-strings" => fixed = true,
            "--extended-regexp" => extended = true,
            "-e" | "--regexp" | "-g" | "--glob" | "-A" | "--after-context" | "-B"
            | "--before-context" | "-C" | "--context" | "--max-results" => {
                skip_next = true;
            }
            value if search_option_has_attached_value(value) => {}
            value if value.starts_with('-') && !value.starts_with("--") => {
                let flags = value.trim_start_matches('-');
                fixed |= flags.contains('F');
                extended |= flags.contains('E');
            }
            _ => {}
        }
    }

    let mode = if files {
        "files"
    } else if fixed {
        "fixed"
    } else if extended {
        "extended"
    } else {
        "basic"
    };
    format!("{root} {mode}")
}

fn search_option_has_attached_value(arg: &str) -> bool {
    [
        "--regexp=",
        "--glob=",
        "--after-context=",
        "--before-context=",
        "--context=",
        "--max-results=",
    ]
    .iter()
    .any(|prefix| arg.starts_with(prefix))
        || ["-e", "-g", "-A", "-B", "-C"]
            .iter()
            .any(|prefix| arg.starts_with(prefix) && arg.len() > prefix.len())
}

fn git_family(args: &[String]) -> String {
    let options_with_values = [
        "-C",
        "-c",
        "--git-dir",
        "--work-tree",
        "--namespace",
        "--exec-path",
        "--config-env",
    ];
    match first_positional(args, &options_with_values, true) {
        Some(
            subcommand @ ("status" | "diff" | "log" | "show" | "evidence-diff" | "conflict-diff"),
        ) => {
            format!("git {subcommand}")
        }
        _ => passthrough_family("git"),
    }
}

fn recognized_subcommand_family(
    root: &str,
    args: &[String],
    supported: &[&str],
    options_with_values: &[&str],
    skip_toolchains: bool,
) -> String {
    if let Some(subcommand) = first_positional(args, options_with_values, skip_toolchains)
        .filter(|value| supported.contains(value))
    {
        format!("{root} {subcommand}")
    } else {
        passthrough_family(root)
    }
}

fn cmake_family(args: &[String]) -> String {
    if args.first().is_some_and(|arg| arg == "build")
        || args
            .iter()
            .any(|arg| arg == "--build" || arg.starts_with("--build="))
    {
        return "cmake build".to_string();
    }
    if args.iter().any(|arg| {
        matches!(arg.as_str(), "-S" | "-B" | "-G" | "--preset")
            || arg.starts_with("-S")
            || arg.starts_with("-B")
            || arg.starts_with("--preset=")
    }) || args.first().is_some_and(|arg| !arg.starts_with('-'))
    {
        return "cmake configure".to_string();
    }
    passthrough_family("cmake")
}

fn node_family(args: &[String]) -> String {
    if super::node_cmd::check_flag_precedes_program(args) {
        "node check".to_string()
    } else if args
        .iter()
        .any(|arg| arg == "test" || arg == "--test" || arg.starts_with("--test="))
    {
        "node test".to_string()
    } else {
        "node run".to_string()
    }
}

fn npm_family(args: &[String]) -> String {
    let value_options = [
        "--prefix",
        "--workspace",
        "-w",
        "--userconfig",
        "--cache",
        "--registry",
    ];
    let Some(index) = first_positional_index(args, &value_options, false) else {
        return passthrough_family("npm");
    };
    match args[index].as_str() {
        "test" | "t" | "tst" => "npm test".to_string(),
        "build" => "npm build".to_string(),
        "ci" | "install" | "i" => "npm install".to_string(),
        "run" | "run-script" => npm_run_family(args.get(index + 1..).unwrap_or_default()),
        _ => passthrough_family("npm"),
    }
}

fn npm_run_family(args: &[String]) -> String {
    let Some(script) = first_positional(args, &[], false) else {
        return "npm run".to_string();
    };
    if script == "test" || script.starts_with("test:") || matches!(script, "unit" | "e2e") {
        "npm test".to_string()
    } else if script == "build" || script.starts_with("build:") || script == "compile" {
        "npm build".to_string()
    } else {
        "npm run".to_string()
    }
}

fn npx_family(args: &[String]) -> String {
    let value_options = ["--package", "-p", "--node-options", "--npm", "--call", "-c"];
    let Some(tool) = first_positional(args, &value_options, false) else {
        return passthrough_family("npx");
    };
    let tool = canonical_program_name(tool);
    if tool == "unknown" {
        passthrough_family("npx")
    } else {
        format!("npx {tool}")
    }
}

fn dotnet_family(args: &[String]) -> String {
    let value_options = ["--arch", "--os", "--runtime", "-r", "--verbosity", "-v"];
    match first_positional(args, &value_options, false) {
        Some(subcommand @ ("build" | "test" | "restore" | "format")) => {
            format!("dotnet {subcommand}")
        }
        _ => passthrough_family("dotnet"),
    }
}

fn clang_format_family(args: &[String]) -> String {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--dry-run" | "--Werror"))
    {
        "clang-format check".to_string()
    } else if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-i" | "--in-place"))
    {
        "clang-format write".to_string()
    } else {
        "clang-format format".to_string()
    }
}

fn first_positional<'a>(
    args: &'a [String],
    options_with_values: &[&str],
    skip_toolchains: bool,
) -> Option<&'a str> {
    first_positional_index(args, options_with_values, skip_toolchains)
        .map(|index| args[index].as_str())
}

fn first_positional_index(
    args: &[String],
    options_with_values: &[&str],
    skip_toolchains: bool,
) -> Option<usize> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return (index + 1 < args.len()).then_some(index + 1);
        }
        if options_with_values.contains(&arg.as_str()) {
            index += 2;
            continue;
        }
        if option_has_attached_value(arg, options_with_values)
            || arg.starts_with('-')
            || (skip_toolchains && arg.starts_with('+'))
        {
            index += 1;
            continue;
        }
        return Some(index);
    }
    None
}

fn option_has_attached_value(arg: &str, options_with_values: &[&str]) -> bool {
    options_with_values.iter().any(|option| {
        option.starts_with("--")
            && arg
                .strip_prefix(option)
                .is_some_and(|suffix| suffix.starts_with('='))
    })
}

fn passthrough_family(root: &str) -> String {
    format!("passthrough {root}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    fn identity(args: &[&str]) -> CommandIdentity {
        CommandIdentity::classify(&strings(args))
    }

    #[test]
    fn removes_cx_and_separator_wrappers_from_canonical_identity() {
        let direct = identity(&["cx", "--", "git", "diff", "--stat"]);
        assert_eq!(direct.argv, strings(&["git", "diff", "--stat"]));
        assert_eq!(direct.root, "git");
        assert_eq!(direct.family, "git diff");

        let repeated = identity(&["--", "cx", "--", "cx", "git", "status"]);
        assert_eq!(repeated.argv, strings(&["git", "status"]));
        assert_eq!(repeated.family, "git status");
    }

    #[test]
    fn decodes_one_reporting_argument_without_executing_shell_syntax() {
        let classified = identity(&["cx -- rg -F -e 'route|path' 'app tests'"]);
        assert_eq!(
            classified.argv,
            strings(&["rg", "-F", "-e", "route|path", "app tests"])
        );
        assert_eq!(classified.family, "rg fixed");

        let malformed = identity(&["cx -- git 'unterminated"]);
        assert_eq!(malformed.family, "unknown");
    }

    #[test]
    fn classifies_search_dialects_without_treating_pattern_values_as_flags() {
        assert_eq!(identity(&["grep", "needle"]).family, "grep basic");
        assert_eq!(identity(&["grep", "-nE", "a|b"]).family, "grep extended");
        assert_eq!(identity(&["grep", "-e", "-F"]).family, "grep basic");
        assert_eq!(identity(&["rg", "needle"]).family, "rg extended");
        assert_eq!(identity(&["rg", "--files"]).family, "rg files");
    }

    #[test]
    fn classifies_supported_and_passthrough_command_shapes() {
        assert_eq!(
            identity(&["git", "-C", "repo", "log", "-5"]).family,
            "git log"
        );
        assert_eq!(identity(&["git", "branch"]).family, "passthrough git");
        assert_eq!(
            identity(&["cargo", "+nightly", "test"]).family,
            "cargo test"
        );
        assert_eq!(
            identity(&["cmake", "--build", "build"]).family,
            "cmake build"
        );
        assert_eq!(
            identity(&["cmake", "-S", ".", "-B", "build"]).family,
            "cmake configure"
        );
        assert_eq!(
            identity(&["docker", "--context", "prod", "ps"]).family,
            "docker ps"
        );
        assert_eq!(identity(&["jq", ".items"]).family, "passthrough jq");
    }

    #[test]
    fn classifies_node_package_and_toolchain_modes() {
        assert_eq!(
            identity(&["node", "--check", "app.js"]).family,
            "node check"
        );
        assert_eq!(identity(&["node", "app.js", "--check"]).family, "node run");
        assert_eq!(
            identity(&["node", "--test", "app.test.js"]).family,
            "node test"
        );
        assert_eq!(identity(&["node", "app.js"]).family, "node run");
        assert_eq!(identity(&["npm", "run", "test:unit"]).family, "npm test");
        assert_eq!(
            identity(&["npm", "--prefix", "web", "run", "build"]).family,
            "npm build"
        );
        assert_eq!(
            identity(&["npx", "--yes", "tsc", "--noEmit"]).family,
            "npx tsc"
        );
        assert_eq!(
            identity(&["dotnet", "test", "app.sln"]).family,
            "dotnet test"
        );
        assert_eq!(
            identity(&["clang-format", "--dry-run", "src/main.cc"]).family,
            "clang-format check"
        );
    }

    #[test]
    fn redacts_key_like_program_names_from_identity() {
        let classified = identity(&["sk-proj-abcdefghijklmnopqrstuvwxyz", "--version"]);
        assert_eq!(classified.root, "unknown");
        assert_eq!(classified.family, "unknown");

        let high_entropy = identity(&["A9qZ_8LmNoP2rStUv", "--version"]);
        assert_eq!(high_entropy.root, "unknown");
        assert_eq!(high_entropy.family, "unknown");
    }

    #[test]
    fn preserves_structured_path_programs_in_passthrough_identity() {
        let relative = identity(&[
            "app/sample-suite/build/sample-suite-tests",
            "--gtest_filter=Planner.*",
        ]);
        let absolute = identity(&[
            "/Users/example/project/app/sample-suite/build/sample-suite-tests",
            "--gtest_filter=Planner.*",
        ]);
        assert_eq!(relative.root, "sample-suite-tests");
        assert_eq!(relative.family, "passthrough sample-suite-tests");
        assert_eq!(absolute.root, relative.root);
        assert_eq!(absolute.family, relative.family);
    }
}
