use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use glob::Pattern;
use walkdir::{DirEntry, Error as WalkError, WalkDir};

use crate::support::runner::ProxyOutcome;

const DEFAULT_MAX_RESULTS: usize = 120;
const MAX_TRAVERSAL_ERRORS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionMatchKind {
    Exact,
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PermissionMatch {
    kind: PermissionMatchKind,
    mode: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryType {
    File,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FindOptions {
    roots: Vec<PathBuf>,
    names: Vec<String>,
    inames: Vec<String>,
    paths: Vec<String>,
    ipaths: Vec<String>,
    entry_type: Option<EntryType>,
    permission: Option<PermissionMatch>,
    max_depth: Option<usize>,
    max_results: usize,
    hidden: bool,
}

pub fn run(args: &[String]) -> Result<ProxyOutcome> {
    let options = parse_find_args(args)?;
    let discovery = discover(&options)?;
    let raw_output = discovery.matches.join("\n");
    let stdout = if discovery.matches.is_empty() && discovery.total_errors > 0 {
        String::new()
    } else {
        format_find_results(&discovery.matches, options.max_results)
    };
    Ok(ProxyOutcome {
        stdout,
        stderr: format_traversal_errors(&discovery),
        exit_code: i32::from(discovery.total_errors > 0),
        observation: None,
    }
    .with_raw_output("find matches", &raw_output)
    .with_expansion_reason("bounded-result-summary"))
}

fn parse_find_args(args: &[String]) -> Result<FindOptions> {
    let mut options = FindOptions {
        roots: Vec::new(),
        names: Vec::new(),
        inames: Vec::new(),
        paths: Vec::new(),
        ipaths: Vec::new(),
        entry_type: None,
        permission: None,
        max_depth: None,
        max_results: DEFAULT_MAX_RESULTS,
        hidden: false,
    };

    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "(" | ")" | "-o" | "-or" | "-a" | "-and" | "--" | "-print" | "-print0" => {
                index += 1;
            }
            "-maxdepth" | "--maxdepth" | "--max-depth" => {
                let value = take_value(args, &mut index, arg)?;
                options.max_depth = Some(parse_positive_usize(value, arg)?);
            }
            "-type" | "--type" => {
                let value = take_value(args, &mut index, arg)?;
                options.entry_type = Some(parse_entry_type(value)?);
            }
            "-perm" | "--perm" => {
                let value = take_value(args, &mut index, arg)?;
                options.permission = Some(parse_permission_match(value)?);
            }
            "-name" | "--name" => {
                let value = take_value(args, &mut index, arg)?;
                options.names.push(value.to_string());
            }
            "-iname" | "--iname" => {
                let value = take_value(args, &mut index, arg)?;
                options.inames.push(value.to_string());
            }
            "-path" | "--path" | "-wholename" | "--wholename" => {
                let value = take_value(args, &mut index, arg)?;
                options.paths.push(normalize_path_pattern(value));
            }
            "-ipath" | "--ipath" | "-iwholename" | "--iwholename" => {
                let value = take_value(args, &mut index, arg)?;
                options.ipaths.push(normalize_path_pattern(value));
            }
            "--hidden" => {
                options.hidden = true;
                index += 1;
            }
            "--max-results" | "--head" => {
                let value = take_value(args, &mut index, arg)?;
                options.max_results = parse_positive_usize(value, arg)?;
            }
            _ if arg.starts_with("--max-results=") => {
                options.max_results = parse_positive_usize(
                    arg.trim_start_matches("--max-results="),
                    "--max-results",
                )?;
                index += 1;
            }
            _ if arg.starts_with("--head=") => {
                options.max_results =
                    parse_positive_usize(arg.trim_start_matches("--head="), "--head")?;
                index += 1;
            }
            _ if arg.starts_with("--maxdepth=") => {
                options.max_depth = Some(parse_positive_usize(
                    arg.trim_start_matches("--maxdepth="),
                    "--maxdepth",
                )?);
                index += 1;
            }
            _ if arg.starts_with("--max-depth=") => {
                options.max_depth = Some(parse_positive_usize(
                    arg.trim_start_matches("--max-depth="),
                    "--max-depth",
                )?);
                index += 1;
            }
            _ if arg.starts_with('-') => bail!("unsupported cx find option `{arg}`"),
            _ => {
                options.roots.push(PathBuf::from(arg));
                index += 1;
            }
        }
    }

    if options.roots.is_empty() {
        options.roots.push(PathBuf::from("."));
    }
    Ok(options)
}

fn take_value<'a>(args: &'a [String], index: &mut usize, option: &str) -> Result<&'a str> {
    *index += 1;
    args.get(*index)
        .map(|value| {
            *index += 1;
            value.as_str()
        })
        .with_context(|| format!("missing value for `{option}`"))
}

fn parse_positive_usize(value: &str, option: &str) -> Result<usize> {
    let parsed = value
        .parse::<usize>()
        .with_context(|| format!("invalid value `{value}` for `{option}`"))?;
    if parsed == 0 {
        bail!("`{option}` must be greater than 0");
    }
    Ok(parsed)
}

fn parse_entry_type(value: &str) -> Result<EntryType> {
    match value {
        "f" | "file" => Ok(EntryType::File),
        "d" | "dir" | "directory" => Ok(EntryType::Dir),
        _ => bail!("unsupported cx find type `{value}`; expected `f` or `d`"),
    }
}

fn parse_permission_match(value: &str) -> Result<PermissionMatch> {
    let (kind, mode_str) = if let Some(mode) = value.strip_prefix('+') {
        (PermissionMatchKind::Any, mode)
    } else if let Some(mode) = value.strip_prefix('/') {
        (PermissionMatchKind::Any, mode)
    } else if let Some(mode) = value.strip_prefix('-') {
        (PermissionMatchKind::All, mode)
    } else {
        (PermissionMatchKind::Exact, value)
    };

    if mode_str.is_empty() {
        bail!("unsupported cx find perm `{value}`; expected octal mode like `+111` or `755`");
    }
    if !mode_str.chars().all(|ch| matches!(ch, '0'..='7')) {
        bail!("unsupported cx find perm `{value}`; only octal modes are supported");
    }

    let mode = u32::from_str_radix(mode_str, 8)
        .with_context(|| format!("invalid permission mode `{value}`"))?;
    Ok(PermissionMatch { kind, mode })
}

fn discover(options: &FindOptions) -> Result<Discovery> {
    let name_patterns = compile_patterns(&options.names)?;
    let iname_patterns = compile_iname_patterns(&options.inames)?;
    let path_patterns = compile_patterns(&options.paths)?;
    let ipath_patterns = compile_iname_patterns(&options.ipaths)?;
    let mut matches = Vec::new();
    let mut errors = Vec::new();
    let mut total_errors = 0usize;

    for root in &options.roots {
        let mut walker = WalkDir::new(root).follow_links(false);
        if let Some(max_depth) = options.max_depth {
            walker = walker.max_depth(max_depth + 1);
        }

        for result in walker
            .into_iter()
            .filter_entry(|entry| options.hidden || !path_is_hidden(entry.path(), root))
        {
            let entry = match result {
                Ok(entry) => entry,
                Err(error) => {
                    record_traversal_error(
                        &mut errors,
                        &mut total_errors,
                        traversal_error(&error, root),
                    );
                    continue;
                }
            };
            let path = entry.path();
            if path == root {
                continue;
            }
            if !entry_matches_type(&entry, options.entry_type) {
                continue;
            }
            let permission_matches = match entry_matches_permission(&entry, options.permission) {
                Ok(matches) => matches,
                Err(error) => {
                    record_traversal_error(&mut errors, &mut total_errors, error);
                    continue;
                }
            };
            if !permission_matches {
                continue;
            }
            if !entry_matches_patterns(
                path,
                &name_patterns,
                &iname_patterns,
                &path_patterns,
                &ipath_patterns,
            ) {
                continue;
            }
            matches.push(path.display().to_string());
        }
    }

    matches.sort();
    matches.dedup();
    Ok(Discovery {
        matches,
        errors,
        total_errors,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Discovery {
    matches: Vec<String>,
    errors: Vec<String>,
    total_errors: usize,
}

fn compile_patterns(patterns: &[String]) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            Pattern::new(pattern).with_context(|| format!("invalid name pattern `{pattern}`"))
        })
        .collect()
}

fn compile_iname_patterns(patterns: &[String]) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            let lowered = pattern.to_ascii_lowercase();
            Pattern::new(&lowered).with_context(|| format!("invalid iname pattern `{pattern}`"))
        })
        .collect()
}

fn entry_matches_type(entry: &DirEntry, entry_type: Option<EntryType>) -> bool {
    match entry_type {
        Some(EntryType::File) => entry.file_type().is_file(),
        Some(EntryType::Dir) => entry.file_type().is_dir(),
        None => true,
    }
}

fn entry_matches_permission(
    entry: &DirEntry,
    permission: Option<PermissionMatch>,
) -> Result<bool, String> {
    let Some(permission) = permission else {
        return Ok(true);
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = entry
            .metadata()
            .map_err(|error| traversal_error(&error, entry.path()))?;
        let mode = metadata.permissions().mode() & 0o7777;
        Ok(match permission.kind {
            PermissionMatchKind::Exact => mode == permission.mode,
            PermissionMatchKind::All => (mode & permission.mode) == permission.mode,
            PermissionMatchKind::Any => (mode & permission.mode) != 0,
        })
    }

    #[cfg(not(unix))]
    {
        let _ = entry;
        let _ = permission;
        Ok(false)
    }
}

fn entry_matches_patterns(
    path: &Path,
    names: &[Pattern],
    inames: &[Pattern],
    paths: &[Pattern],
    ipaths: &[Pattern],
) -> bool {
    if names.is_empty() && inames.is_empty() && paths.is_empty() && ipaths.is_empty() {
        return true;
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let normalized_path = normalize_path_string(path);
    let name_matches = names.is_empty() && inames.is_empty()
        || names.iter().any(|pattern| pattern.matches(file_name))
        || inames
            .iter()
            .any(|pattern| pattern.matches(&file_name.to_ascii_lowercase()));
    let path_matches = paths.is_empty() && ipaths.is_empty()
        || paths
            .iter()
            .any(|pattern| pattern.matches(&normalized_path))
        || ipaths
            .iter()
            .any(|pattern| pattern.matches(&normalized_path.to_ascii_lowercase()));
    name_matches && path_matches
}

fn path_is_hidden(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|part| part.starts_with('.') && part.len() > 1)
    })
}

fn record_traversal_error(errors: &mut Vec<String>, total_errors: &mut usize, error: String) {
    *total_errors += 1;
    let index = match errors.binary_search(&error) {
        Ok(_) => return,
        Err(index) => index,
    };
    if errors.len() < MAX_TRAVERSAL_ERRORS {
        errors.insert(index, error);
    } else if index < MAX_TRAVERSAL_ERRORS {
        errors.insert(index, error);
        errors.truncate(MAX_TRAVERSAL_ERRORS);
    }
}

fn traversal_error(error: &WalkError, fallback_path: &Path) -> String {
    let path = error.path().unwrap_or(fallback_path);
    let reason = error
        .io_error()
        .map(|error| match error.kind() {
            std::io::ErrorKind::NotFound => "not found".to_string(),
            std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
            _ => error.to_string(),
        })
        .unwrap_or_else(|| "traversal failed".to_string());
    format!("find: {}: {reason}", path.display())
}

fn format_traversal_errors(discovery: &Discovery) -> String {
    if discovery.total_errors == 0 {
        return String::new();
    }
    let mut output = format!(
        "find: incomplete traversal ({} error{})\n",
        discovery.total_errors,
        if discovery.total_errors == 1 { "" } else { "s" }
    );
    for error in &discovery.errors {
        output.push_str(error);
        output.push('\n');
    }
    let hidden = discovery
        .total_errors
        .saturating_sub(discovery.errors.len());
    if hidden > 0 {
        output.push_str(&format!("find: ... +{hidden} more traversal errors\n"));
    }
    output.trim_end().to_string()
}

fn format_find_results(matches: &[String], max_results: usize) -> String {
    if matches.is_empty() {
        return "find: 0 entries".to_string();
    }
    let shown_count = max_results.min(matches.len());
    let mut result = if shown_count == matches.len() {
        format!("find: {} entries\n", matches.len())
    } else {
        format!("find: {shown_count} shown of {} entries\n", matches.len())
    };
    for path in matches.iter().take(shown_count) {
        result.push_str(path);
        result.push('\n');
    }
    if shown_count < matches.len() {
        result.push_str(&format!(
            "... +{} more entries hidden by --max-results\n",
            matches.len() - shown_count
        ));
    }
    result.trim_end().to_string()
}

fn normalize_path_pattern(pattern: &str) -> String {
    pattern.replace('\\', "/")
}

fn normalize_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parse_find_args_accepts_find_style_options() {
        let options = parse_find_args(&[
            "src".to_string(),
            "-maxdepth".to_string(),
            "3".to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-perm".to_string(),
            "+111".to_string(),
            "(".to_string(),
            "-name".to_string(),
            "*.json".to_string(),
            "-o".to_string(),
            "-iname".to_string(),
            "*.PNG".to_string(),
            "-path".to_string(),
            "*/assets/*.json".to_string(),
            "-o".to_string(),
            "-ipath".to_string(),
            "*/PACKAGES/*.PNG".to_string(),
            ")".to_string(),
            "--max-results".to_string(),
            "20".to_string(),
        ])
        .unwrap();

        assert_eq!(options.roots, vec![PathBuf::from("src")]);
        assert_eq!(options.max_depth, Some(3));
        assert_eq!(options.entry_type, Some(EntryType::File));
        assert_eq!(
            options.permission,
            Some(PermissionMatch {
                kind: PermissionMatchKind::Any,
                mode: 0o111,
            })
        );
        assert_eq!(options.names, vec!["*.json".to_string()]);
        assert_eq!(options.inames, vec!["*.PNG".to_string()]);
        assert_eq!(options.paths, vec!["*/assets/*.json".to_string()]);
        assert_eq!(options.ipaths, vec!["*/PACKAGES/*.PNG".to_string()]);
        assert_eq!(options.max_results, 20);
    }

    #[test]
    fn parse_find_args_accepts_all_and_exact_perm_modes() {
        let all =
            parse_find_args(&[".".to_string(), "-perm".to_string(), "-755".to_string()]).unwrap();
        assert_eq!(
            all.permission,
            Some(PermissionMatch {
                kind: PermissionMatchKind::All,
                mode: 0o755,
            })
        );

        let exact =
            parse_find_args(&[".".to_string(), "-perm".to_string(), "644".to_string()]).unwrap();
        assert_eq!(
            exact.permission,
            Some(PermissionMatch {
                kind: PermissionMatchKind::Exact,
                mode: 0o644,
            })
        );
    }

    #[test]
    fn run_finds_bounded_sorted_files() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join("docs/.hidden")).unwrap();
        fs::write(root.join("docs/b.summary.json"), "{}\n").unwrap();
        fs::write(root.join("docs/a.runtime.json"), "{}\n").unwrap();
        fs::write(root.join("docs/.hidden/c.summary.json"), "{}\n").unwrap();
        fs::write(root.join("docs/ignore.txt"), "x\n").unwrap();

        let output = run(&[
            root.join("docs").display().to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-name".to_string(),
            "*.json".to_string(),
            "--max-results".to_string(),
            "1".to_string(),
        ])
        .unwrap();

        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("1 shown of 2 entries"));
        assert!(output.stdout.contains("a.runtime.json"));
        assert!(!output.stdout.contains(".hidden"));
        assert!(output.stdout.contains("+1 more entries"));
    }

    #[test]
    fn run_supports_hidden_and_iname() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::write(root.join(".cache/Proof.PNG"), "x\n").unwrap();

        let output = run(&[
            root.display().to_string(),
            "--hidden".to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-iname".to_string(),
            "*.png".to_string(),
        ])
        .unwrap();

        assert!(output.stdout.contains("Proof.PNG"));
    }

    #[test]
    fn run_reports_missing_roots_as_incomplete_traversal() {
        let temp = tempdir().unwrap();
        let missing = temp.path().join("missing");

        let output = run(&[missing.display().to_string()]).unwrap();

        assert_eq!(output.exit_code, 1);
        assert!(output.stdout.is_empty());
        assert!(output
            .stderr
            .contains("find: incomplete traversal (1 error)"));
        assert!(output.stderr.contains(&missing.display().to_string()));
        assert!(output.stderr.contains("not found"));
    }

    #[test]
    fn run_preserves_partial_matches_when_another_root_is_missing() {
        let temp = tempdir().unwrap();
        let valid = temp.path().join("valid");
        let missing = temp.path().join("missing");
        fs::create_dir_all(&valid).unwrap();
        fs::write(valid.join("proof.txt"), "proof\n").unwrap();

        let output = run(&[
            valid.display().to_string(),
            missing.display().to_string(),
            "-type".to_string(),
            "f".to_string(),
        ])
        .unwrap();

        assert_eq!(output.exit_code, 1);
        assert!(output.stdout.contains("find: 1 entries"));
        assert!(output.stdout.contains("proof.txt"));
        assert!(output.stderr.contains("incomplete traversal"));
        assert!(output.stderr.contains(&missing.display().to_string()));
    }

    #[test]
    fn traversal_error_output_is_bounded() {
        let discovery = Discovery {
            matches: Vec::new(),
            errors: (0..MAX_TRAVERSAL_ERRORS)
                .map(|index| format!("find: missing-{index}: not found"))
                .collect(),
            total_errors: MAX_TRAVERSAL_ERRORS + 7,
        };

        let output = format_traversal_errors(&discovery);

        assert!(output.contains("27 errors"));
        assert!(output.contains("missing-0"));
        assert!(output.contains("+7 more traversal errors"));
    }

    #[test]
    fn run_supports_path_predicate_for_nested_directories() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join("app/node_modules/playwright")).unwrap();
        fs::create_dir_all(root.join("app/node_modules/other")).unwrap();

        let output = run(&[
            root.display().to_string(),
            "-maxdepth".to_string(),
            "4".to_string(),
            "-type".to_string(),
            "d".to_string(),
            "-path".to_string(),
            "*/node_modules/playwright".to_string(),
        ])
        .unwrap();

        assert!(output.stdout.contains("node_modules/playwright"));
        assert!(!output.stdout.contains("node_modules/other"));
    }

    #[test]
    fn run_combines_filename_and_path_predicate_groups() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join("build/debug")).unwrap();
        fs::create_dir_all(root.join("source")).unwrap();
        fs::write(root.join("build/debug/sample-service"), "binary\n").unwrap();
        fs::write(root.join("build/debug/other-service"), "binary\n").unwrap();
        fs::write(root.join("source/sample-service"), "source\n").unwrap();

        let output = run(&[
            root.display().to_string(),
            "-path".to_string(),
            "*/build/*".to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-name".to_string(),
            "sample-service".to_string(),
        ])
        .unwrap();

        assert!(output.stdout.contains("build/debug/sample-service"));
        assert!(!output.stdout.contains("build/debug/other-service"));
        assert!(!output.stdout.contains("source/sample-service"));
    }

    #[test]
    fn run_combines_path_and_case_insensitive_filename_predicates() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join("actor/captures")).unwrap();
        fs::create_dir_all(root.join("other/captures")).unwrap();
        fs::write(root.join("actor/captures/HERO-BASELINE.PNG"), "image\n").unwrap();
        fs::write(root.join("actor/captures/current.png"), "image\n").unwrap();
        fs::write(root.join("other/captures/HERO-BASELINE.PNG"), "image\n").unwrap();

        let output = run(&[
            root.display().to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-path".to_string(),
            "*actor*".to_string(),
            "-iname".to_string(),
            "*baseline*".to_string(),
        ])
        .unwrap();

        assert!(output.stdout.contains("actor/captures/HERO-BASELINE.PNG"));
        assert!(!output.stdout.contains("actor/captures/current.png"));
        assert!(!output.stdout.contains("other/captures/HERO-BASELINE.PNG"));
    }

    #[test]
    fn run_supports_ipath_predicate_case_insensitively() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join("App/Node_Modules/PlayWright")).unwrap();

        let output = run(&[
            root.display().to_string(),
            "-type".to_string(),
            "d".to_string(),
            "-ipath".to_string(),
            "*/node_modules/playwright".to_string(),
        ])
        .unwrap();

        assert!(output.stdout.contains("PlayWright"));
    }

    #[cfg(unix)]
    #[test]
    fn run_filters_by_any_executable_permission_bit() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(&root).unwrap();

        let exec = root.join("alpha-test-bin");
        let plain = root.join("beta-test-bin");
        fs::write(&exec, "#!/bin/sh\n").unwrap();
        fs::write(&plain, "plain\n").unwrap();
        fs::set_permissions(&exec, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();

        let output = run(&[
            root.display().to_string(),
            "-maxdepth".to_string(),
            "3".to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-perm".to_string(),
            "+111".to_string(),
            "-name".to_string(),
            "*test*".to_string(),
        ])
        .unwrap();

        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("alpha-test-bin"));
        assert!(!output.stdout.contains("beta-test-bin"));
    }
}
