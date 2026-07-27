use std::collections::VecDeque;
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

use anyhow::{anyhow, Result};

pub fn truncate(value: &str, max_len: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_len {
        value.to_string()
    } else if max_len <= 3 {
        "...".to_string()
    } else {
        let truncated: String = value.chars().take(max_len - 3).collect();
        format!("{truncated}...")
    }
}

pub fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '/' | '.' | '_' | '-' | '=' | ':' | '@' | ',' | '+' | '%'
                )
        })
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

pub fn fallback_window(output: &str, head_lines: usize, tail_lines: usize) -> String {
    if head_lines == 0 && tail_lines == 0 {
        return String::new();
    }

    let mut head = Vec::with_capacity(head_lines);
    let mut tail = VecDeque::with_capacity(tail_lines);
    let mut line_count = 0usize;

    for line in output.lines() {
        line_count += 1;
        if head.len() < head_lines {
            head.push(line);
        }
        if tail_lines > 0 {
            if tail.len() == tail_lines {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    }

    if line_count <= head_lines + tail_lines {
        return join_lines(output.lines());
    }

    let mut result = String::new();
    append_lines(&mut result, head);
    append_line(
        &mut result,
        &format!(
            "... [{} lines omitted] ...",
            line_count - head_lines - tail_lines
        ),
    );
    append_lines(&mut result, tail);
    result
}

pub fn fallback_tail(output: &str, lines: usize) -> String {
    if lines == 0 {
        return String::new();
    }
    let mut tail = VecDeque::with_capacity(lines);
    for line in output.lines() {
        if tail.len() == lines {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    join_lines(tail)
}

fn join_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut output = String::new();
    append_lines(&mut output, lines);
    output
}

fn append_lines<'a>(output: &mut String, lines: impl IntoIterator<Item = &'a str>) {
    for line in lines {
        append_line(output, line);
    }
}

fn append_line(output: &mut String, line: &str) {
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(line);
}

pub fn resolve_binary(name: &str) -> Result<PathBuf> {
    if let Some(path) = resolve_from_preferred_override_dirs(name) {
        return Ok(path);
    }
    if let Ok(path) = which::which(name) {
        return Ok(path);
    }
    if let Some(path) = resolve_from_fallback_dirs(name) {
        return Ok(path);
    }
    Err(anyhow!(
        "binary `{name}` not found on PATH or fallback tool directories"
    ))
}

pub fn resolved_command(name: &str) -> Command {
    match resolve_binary(name) {
        Ok(path) => Command::new(path),
        Err(_) => Command::new(name),
    }
}

pub fn tool_exists(name: &str) -> bool {
    resolve_binary(name).is_ok()
}

fn resolve_from_fallback_dirs(name: &str) -> Option<PathBuf> {
    if fallback_dir_lookup_disabled() {
        return None;
    }

    fallback_search_dirs()
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn resolve_from_preferred_override_dirs(name: &str) -> Option<PathBuf> {
    if fallback_dir_lookup_disabled() {
        return None;
    }
    preferred_override_search_dirs(name)
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn fallback_dir_lookup_disabled() -> bool {
    env::var("CX_DISABLE_TOOL_FALLBACK_PATHS")
        .ok()
        .is_some_and(|value| matches_truthy(&value))
}

fn matches_truthy(value: &str) -> bool {
    let normalized = value.trim();
    !normalized.is_empty()
        && !normalized.eq_ignore_ascii_case("0")
        && !normalized.eq_ignore_ascii_case("false")
        && !normalized.eq_ignore_ascii_case("no")
        && !normalized.eq_ignore_ascii_case("off")
}

fn fallback_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(extra_paths) = env::var_os("CX_TOOL_FALLBACK_PATHS") {
        dirs.extend(env::split_paths(&extra_paths));
    }

    #[cfg(target_os = "macos")]
    {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/local/bin"),
            PathBuf::from("/Library/Developer/CommandLineTools/usr/bin"),
            PathBuf::from("/Applications/CMake.app/Contents/bin"),
            PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"),
        ]);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs.extend([
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/local/sbin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/local/bin"),
            PathBuf::from("/snap/bin"),
        ]);
    }

    #[cfg(windows)]
    {
        if let Ok(program_files) = env::var("ProgramFiles") {
            dirs.push(PathBuf::from(&program_files).join("CMake").join("bin"));
            dirs.push(
                PathBuf::from(&program_files)
                    .join("Docker")
                    .join("Docker")
                    .join("resources")
                    .join("bin"),
            );
        }
        if let Ok(program_files_x86) = env::var("ProgramFiles(x86)") {
            dirs.push(PathBuf::from(&program_files_x86).join("CMake").join("bin"));
        }
    }

    let mut deduped = Vec::new();
    for dir in dirs {
        if dir.as_os_str().is_empty() || !dir.exists() || deduped.contains(&dir) {
            continue;
        }
        deduped.push(dir);
    }
    deduped
}

fn preferred_override_search_dirs(name: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(extra_paths) = env::var_os("CX_TOOL_OVERRIDE_PATHS") {
        dirs.extend(env::split_paths(&extra_paths));
    }

    if prefers_user_local_override(name) {
        if let Some(home) = env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local").join("bin"));
        }
    }

    let mut deduped = Vec::new();
    for dir in dirs {
        if dir.as_os_str().is_empty() || !dir.exists() || deduped.contains(&dir) {
            continue;
        }
        deduped.push(dir);
    }
    deduped
}

fn prefers_user_local_override(name: &str) -> bool {
    matches!(name, "ffmpeg" | "ffprobe")
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        fs::metadata(path)
            .map(|metadata| metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0))
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        if path.is_file() {
            return true;
        }
        ["exe", "cmd", "bat"]
            .into_iter()
            .map(|extension| path.with_extension(extension))
            .any(|candidate| candidate.is_file())
    }
}

pub fn exit_code_from_output(output: &Output, label: &str) -> i32 {
    match output.status.code() {
        Some(code) => code,
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = output.status.signal() {
                    eprintln!("cx {label}: process terminated by signal {signal}");
                    return 128 + signal;
                }
            }
            eprintln!("cx {label}: process terminated by signal");
            1
        }
    }
}

pub fn exit_code_from_status(status: &ExitStatus, label: &str) -> i32 {
    match status.code() {
        Some(code) => code,
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = status.signal() {
                    eprintln!("cx {label}: process terminated by signal {signal}");
                    return 128 + signal;
                }
            }
            eprintln!("cx {label}: process terminated by signal");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn truncate_preserves_short_values() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_shortens_long_values() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn fallback_tail_returns_last_lines() {
        let value = "a\nb\nc\nd";
        assert_eq!(fallback_tail(value, 2), "c\nd");
    }

    #[test]
    fn fallback_tail_zero_lines_returns_empty() {
        assert_eq!(fallback_tail("a\nb\nc", 0), "");
    }

    #[test]
    fn fallback_tail_does_not_preserve_trailing_newline() {
        assert_eq!(fallback_tail("a\nb\n", 5), "a\nb");
    }

    #[test]
    fn fallback_window_keeps_head_and_tail() {
        let value = "1\n2\n3\n4\n5\n6";
        let rendered = fallback_window(value, 2, 2);
        assert_eq!(rendered, "1\n2\n... [2 lines omitted] ...\n5\n6");
    }

    #[test]
    fn fallback_window_allows_head_or_tail_only() {
        let value = "1\n2\n3\n4\n5";
        assert_eq!(
            fallback_window(value, 2, 0),
            "1\n2\n... [3 lines omitted] ..."
        );
        assert_eq!(
            fallback_window(value, 0, 2),
            "... [3 lines omitted] ...\n4\n5"
        );
    }

    #[test]
    fn fallback_window_returns_full_output_when_short() {
        let value = "1\n2\n3";
        assert_eq!(fallback_window(value, 2, 2), value);
    }

    #[test]
    fn fallback_window_does_not_preserve_trailing_newline_when_short() {
        assert_eq!(fallback_window("1\n2\n", 2, 2), "1\n2");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_binary_uses_fallback_dirs_when_path_is_missing_tool() {
        let temp = tempdir().unwrap();
        let tool_path = temp.path().join("cmake");
        fs::write(&tool_path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&tool_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tool_path, permissions).unwrap();

        crate::support::test_support::with_fake_path_only(&[], || {
            env::set_var("CX_DISABLE_TOOL_FALLBACK_PATHS", "0");
            env::set_var("CX_TOOL_FALLBACK_PATHS", temp.path());
            let resolved = resolve_binary("cmake").unwrap();
            assert_eq!(resolved, tool_path);
            assert!(tool_exists("cmake"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn resolve_binary_prefers_user_local_ffmpeg_override_before_path() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        let user_bin = home.join(".local").join("bin");
        let path_bin = temp.path().join("path-bin");
        fs::create_dir_all(&user_bin).unwrap();
        fs::create_dir_all(&path_bin).unwrap();
        let override_path = user_bin.join("ffmpeg");
        let path_ffmpeg = path_bin.join("ffmpeg");
        write_executable(&override_path);
        write_executable(&path_ffmpeg);

        crate::support::test_support::with_env_vars(
            &[
                ("HOME", Some(home.to_string_lossy().as_ref())),
                ("PATH", Some(path_bin.to_string_lossy().as_ref())),
                ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("0")),
                ("CX_TOOL_OVERRIDE_PATHS", None),
            ],
            || {
                let resolved = resolve_binary("ffmpeg").unwrap();
                assert_eq!(resolved, override_path);
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_binary_can_disable_user_local_ffmpeg_override() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        let user_bin = home.join(".local").join("bin");
        let path_bin = temp.path().join("path-bin");
        fs::create_dir_all(&user_bin).unwrap();
        fs::create_dir_all(&path_bin).unwrap();
        let override_path = user_bin.join("ffmpeg");
        let path_ffmpeg = path_bin.join("ffmpeg");
        write_executable(&override_path);
        write_executable(&path_ffmpeg);

        crate::support::test_support::with_env_vars(
            &[
                ("HOME", Some(home.to_string_lossy().as_ref())),
                ("PATH", Some(path_bin.to_string_lossy().as_ref())),
                ("CX_DISABLE_TOOL_FALLBACK_PATHS", Some("1")),
                ("CX_TOOL_OVERRIDE_PATHS", None),
            ],
            || {
                let resolved = resolve_binary("ffmpeg").unwrap();
                assert_eq!(resolved, path_ffmpeg);
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_binary_can_disable_fallback_dirs() {
        let temp = tempdir().unwrap();
        let tool_path = temp.path().join("cmake");
        fs::write(&tool_path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&tool_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tool_path, permissions).unwrap();

        crate::support::test_support::with_fake_path_only(&[], || {
            env::set_var("CX_TOOL_FALLBACK_PATHS", temp.path());
            env::set_var("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
            let error = resolve_binary("cmake").unwrap_err();
            assert!(error.to_string().contains("binary `cmake` not found"));
            assert!(!tool_exists("cmake"));
        });
    }

    #[cfg(unix)]
    fn write_executable(path: &std::path::Path) {
        fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}
