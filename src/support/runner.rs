use std::env;
use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::support::capture_cache::{CaptureInputLease, CaptureLease};
use crate::support::failure_artifact::{
    store_failure_artifact, store_failure_artifact_from_files, FailureArtifact,
};
use crate::support::insights::{OutputObservation, TextMetrics};
use crate::support::paths::global_cache_root;
use crate::support::utils::{exit_code_from_status, fallback_window};

const DEFAULT_MAX_FAILURE_CAPTURE_MEMORY_BYTES: u64 = 8 * 1024 * 1024;
const CAPTURE_PREVIEW_HEAD_BYTES: usize = 64 * 1024;
const CAPTURE_PREVIEW_TAIL_BYTES: usize = 256 * 1024;
pub(crate) const FAILURE_ARTIFACT_EXPANSION_REASON: &str = "failure-artifact-recovery-hint";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyOutcome {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub observation: Option<OutputObservation>,
}

impl ProxyOutcome {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            observation: None,
        }
    }

    pub fn with_raw_output(mut self, source: impl Into<String>, raw_output: &str) -> Self {
        self.observation = Some(OutputObservation::from_text(source, raw_output));
        self
    }

    pub fn with_observation(mut self, observation: OutputObservation) -> Self {
        self.observation = Some(observation);
        self
    }

    pub fn with_expansion_reason(mut self, reason: impl Into<String>) -> Self {
        if let Some(observation) = &mut self.observation {
            observation.expansion_reason = Some(reason.into());
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub combined: String,
    pub exit_code: i32,
    raw_observation: OutputObservation,
    failure_artifact: Option<FailureArtifact>,
}

impl CommandOutput {
    pub fn new(
        stdout: impl Into<String>,
        stderr: impl Into<String>,
        combined: impl Into<String>,
        exit_code: i32,
    ) -> Self {
        let stdout = stdout.into();
        let stderr = stderr.into();
        let combined = combined.into();
        Self {
            raw_observation: OutputObservation::from_text("captured output", &combined),
            stdout,
            stderr,
            combined,
            exit_code,
            failure_artifact: None,
        }
    }

    pub fn from_combined(combined: impl Into<String>, exit_code: i32) -> Self {
        Self::new(String::new(), String::new(), combined, exit_code)
    }

    pub fn observation(&self, source: impl Into<String>) -> OutputObservation {
        let mut observation = self.raw_observation.clone();
        observation.source = source.into();
        observation
    }

    pub fn failure_artifact_hint(&mut self, tool_name: &str) -> Option<String> {
        self.failure_artifact_hint_with_stdout_prefix(tool_name, "")
    }

    pub fn failure_artifact_hint_with_stdout_prefix(
        &mut self,
        tool_name: &str,
        stdout_prefix: &str,
    ) -> Option<String> {
        if self.exit_code == 0 {
            return None;
        }
        if self.failure_artifact.is_none() {
            let stdout = if stdout_prefix.trim().is_empty() {
                self.stdout.clone()
            } else if self.stdout.trim().is_empty() {
                stdout_prefix.trim_end().to_string()
            } else {
                format!("{}\n{}", stdout_prefix.trim_end(), self.stdout)
            };
            if stdout.is_empty() && self.stderr.is_empty() {
                return None;
            }
            self.failure_artifact = store_failure_artifact(tool_name, &stdout, &self.stderr).ok();
        }
        let hint = self
            .failure_artifact
            .as_ref()
            .map(|artifact| format!("[full output: {}]", artifact.display_path));
        if hint.is_some() && self.raw_observation.expansion_reason.is_none() {
            self.raw_observation.expansion_reason =
                Some(FAILURE_ARTIFACT_EXPANSION_REASON.to_string());
        }
        hint
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOptions {
    pub filter_stdout_only: bool,
    pub skip_filter_on_failure: bool,
    pub fallback_head_lines: usize,
    pub fallback_tail_lines: usize,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            filter_stdout_only: false,
            skip_filter_on_failure: false,
            fallback_head_lines: 12,
            fallback_tail_lines: 28,
        }
    }
}

impl RunOptions {
    pub fn stdout_only() -> Self {
        Self {
            filter_stdout_only: true,
            ..Self::default()
        }
    }

    pub fn early_exit_on_failure(mut self) -> Self {
        self.skip_filter_on_failure = true;
        self
    }

    pub fn fallback_window(mut self, head_lines: usize, tail_lines: usize) -> Self {
        self.fallback_head_lines = head_lines;
        self.fallback_tail_lines = tail_lines;
        self
    }
}

pub fn capture(cmd: Command, tool_name: &str) -> Result<CommandOutput> {
    capture_with_stdin(cmd, tool_name, Stdio::null())
}

pub fn capture_with_inherited_stdin(cmd: Command, tool_name: &str) -> Result<CommandOutput> {
    capture_with_stdin(cmd, tool_name, Stdio::inherit())
}

pub(crate) struct CapturedStdin {
    lease: CaptureInputLease,
}

pub(crate) fn capture_stdin_if_present() -> Result<Option<CapturedStdin>> {
    if io::stdin().is_terminal() {
        return Ok(None);
    }

    let root = capture_root();
    let unique = format!("stdin-{}-{}", std::process::id(), now_ns());
    let (lease, mut spool) = CaptureInputLease::create(&root, &unique)?;
    let mut stdin = io::stdin().lock();
    let bytes = io::copy(&mut stdin, &mut spool).context("failed to capture redirected stdin")?;
    spool.flush().context("failed to flush captured stdin")?;
    drop(spool);
    if bytes == 0 {
        return Ok(None);
    }
    Ok(Some(CapturedStdin { lease }))
}

pub(crate) fn capture_with_captured_stdin(
    cmd: Command,
    tool_name: &str,
    stdin: &CapturedStdin,
) -> Result<CommandOutput> {
    let file = File::open(stdin.lease.stdin_path()).with_context(|| {
        format!(
            "failed to reopen captured stdin {}",
            stdin.lease.stdin_path().display()
        )
    })?;
    capture_with_stdin(cmd, tool_name, Stdio::from(file))
}

pub(crate) fn capture_with_piped_stdin(cmd: Command, tool_name: &str) -> Result<CommandOutput> {
    if io::stdin().is_terminal() {
        capture(cmd, tool_name)
    } else {
        capture_with_inherited_stdin(cmd, tool_name)
    }
}

fn capture_with_stdin(mut cmd: Command, tool_name: &str, stdin: Stdio) -> Result<CommandOutput> {
    let (capture_files, stdout_writer, stderr_writer) = CaptureFiles::create(tool_name)?;

    cmd.stdin(stdin);
    cmd.stdout(Stdio::from(stdout_writer));
    cmd.stderr(Stdio::from(stderr_writer));

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to run `{tool_name}`"))?;
    let status = child
        .wait()
        .with_context(|| format!("failed to wait for `{tool_name}`"))?;

    capture_files.materialize(exit_code_from_status(&status, tool_name), tool_name)
}

pub fn capture_with_stdin_timeout(
    mut cmd: Command,
    tool_name: &str,
    stdin_bytes: Vec<u8>,
    timeout: Duration,
) -> Result<CommandOutput> {
    let (capture_files, stdout_writer, stderr_writer) = CaptureFiles::create(tool_name)?;

    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::from(stdout_writer));
    cmd.stderr(Stdio::from(stderr_writer));

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to run `{tool_name}`"))?;
    let mut stdin = child
        .stdin
        .take()
        .with_context(|| format!("missing `{tool_name}` stdin"))?;
    let stdin_tool_name = tool_name.to_string();
    let stdin_thread = thread::spawn(move || -> Result<()> {
        stdin
            .write_all(&stdin_bytes)
            .with_context(|| format!("failed to write `{stdin_tool_name}` stdin"))?;
        stdin
            .flush()
            .with_context(|| format!("failed to flush `{stdin_tool_name}` stdin"))?;
        Ok(())
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("failed to poll `{tool_name}`"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdin_thread.join();
            anyhow::bail!("`{tool_name}` timed out after {}ms", timeout.as_millis());
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdin_result = stdin_thread
        .join()
        .map_err(|_| anyhow::anyhow!("`{tool_name}` stdin thread panicked"))?;
    let exit_code = exit_code_from_status(&status, tool_name);
    if exit_code == 0 {
        stdin_result?;
    }

    capture_files.materialize(exit_code, tool_name)
}

pub fn run_filtered<F>(
    cmd: Command,
    tool_name: &str,
    filter_fn: F,
    options: RunOptions,
) -> Result<ProxyOutcome>
where
    F: Fn(&CommandOutput) -> Option<String>,
{
    let mut captured = capture(cmd, tool_name)?;
    let tee_hint = if captured.exit_code == 0 {
        None
    } else {
        captured.failure_artifact_hint(tool_name)
    };
    let observation = captured.observation(tool_name);

    if options.skip_filter_on_failure && captured.exit_code != 0 {
        return Ok(ProxyOutcome {
            stdout: append_failure_hint(failure_stdout(&captured, options), tee_hint.as_deref()),
            stderr: failure_stderr(&captured),
            exit_code: captured.exit_code,
            observation: None,
        }
        .with_observation(observation));
    }

    let stdout = filter_fn(&captured).unwrap_or_else(|| fallback_output(&captured, options));
    let stderr = if options.filter_stdout_only {
        captured.stderr.trim_end().to_string()
    } else {
        String::new()
    };

    Ok(ProxyOutcome {
        stdout: append_failure_hint(stdout, tee_hint.as_deref()),
        stderr,
        exit_code: captured.exit_code,
        observation: None,
    }
    .with_observation(observation))
}

pub(crate) fn failure_artifact_hint(tool_name: &str, stdout: &str, stderr: &str) -> Option<String> {
    if stdout.is_empty() && stderr.is_empty() {
        return None;
    }
    store_failure_artifact(tool_name, stdout, stderr)
        .ok()
        .map(|artifact| format!("[full output: {}]", artifact.display_path))
}

fn failure_stdout(captured: &CommandOutput, options: RunOptions) -> String {
    if options.filter_stdout_only {
        if !captured.stdout.trim().is_empty() {
            fallback_window(
                &captured.stdout,
                options.fallback_head_lines,
                options.fallback_tail_lines,
            )
        } else if captured.stderr.trim().is_empty() {
            fallback_window(
                &captured.combined,
                options.fallback_head_lines,
                options.fallback_tail_lines,
            )
        } else {
            String::new()
        }
    } else if !captured.stdout.trim().is_empty() {
        fallback_window(
            &captured.stdout,
            options.fallback_head_lines,
            options.fallback_tail_lines,
        )
    } else {
        String::new()
    }
}

fn failure_stderr(captured: &CommandOutput) -> String {
    captured.stderr.trim_end().to_string()
}

fn fallback_output(captured: &CommandOutput, options: RunOptions) -> String {
    let source = if options.filter_stdout_only {
        if !captured.stdout.trim().is_empty() {
            captured.stdout.as_str()
        } else if !captured.stderr.trim().is_empty() {
            captured.stderr.as_str()
        } else {
            captured.combined.as_str()
        }
    } else {
        captured.combined.as_str()
    };
    fallback_window(
        source,
        options.fallback_head_lines,
        options.fallback_tail_lines,
    )
}

pub(crate) fn append_failure_hint(stdout: String, hint: Option<&str>) -> String {
    let Some(hint) = hint else {
        return stdout;
    };
    if stdout.trim().is_empty() {
        return hint.to_string();
    }
    format!("{}\n{}", stdout.trim_end(), hint)
}

struct CaptureFiles {
    lease: CaptureLease,
}

impl CaptureFiles {
    fn create(tool_name: &str) -> Result<(Self, File, File)> {
        let root = capture_root();
        let unique = format!(
            "{}-{}-{}",
            sanitize_tool_name(tool_name),
            std::process::id(),
            now_ns()
        );
        let (lease, stdout, stderr) = CaptureLease::create(&root, &unique)?;

        Ok((Self { lease }, stdout, stderr))
    }

    fn materialize(&self, exit_code: i32, tool_name: &str) -> Result<CommandOutput> {
        let stdout_path = self.lease.stdout_path();
        let stderr_path = self.lease.stderr_path();
        let stdout_bytes = file_len(stdout_path)?;
        let stderr_bytes = file_len(stderr_path)?;
        let should_bound_failure = exit_code != 0
            && stdout_bytes.saturating_add(stderr_bytes) > max_failure_capture_memory_bytes();

        let failure_artifact = if should_bound_failure {
            store_failure_artifact_from_files(tool_name, stdout_path, stderr_path).ok()
        } else {
            None
        };
        let bounded = should_bound_failure && failure_artifact.is_some();
        let stdout = if bounded {
            read_capture_preview(stdout_path)?
        } else {
            read_capture_lossy(stdout_path)?
        };
        let stderr = if bounded {
            read_capture_preview(stderr_path)?
        } else {
            read_capture_lossy(stderr_path)?
        };
        let combined = combine_output(&stdout, &stderr);
        let metrics = if bounded {
            combined_capture_metrics(stdout_path, stderr_path)?
        } else {
            TextMetrics::from_text(&combined)
        };
        Ok(CommandOutput {
            raw_observation: OutputObservation::from_metrics_with_response(
                "captured output",
                metrics,
                &combined,
            ),
            stdout,
            stderr,
            combined,
            exit_code,
            failure_artifact,
        })
    }
}

fn capture_root() -> PathBuf {
    global_cache_root()
        .unwrap_or_else(|_| env::temp_dir().join("cx").join("cache"))
        .join("capture")
}

fn read_capture_lossy(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_capture_preview(path: &Path) -> Result<String> {
    let bytes = file_len(path)?;
    let preview_capacity = CAPTURE_PREVIEW_HEAD_BYTES + CAPTURE_PREVIEW_TAIL_BYTES;
    if bytes <= preview_capacity as u64 {
        return read_capture_lossy(path);
    }

    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut head = vec![0u8; CAPTURE_PREVIEW_HEAD_BYTES];
    file.read_exact(&mut head)
        .with_context(|| format!("failed to read {}", path.display()))?;
    file.seek(SeekFrom::End(-(CAPTURE_PREVIEW_TAIL_BYTES as i64)))
        .with_context(|| format!("failed to seek {}", path.display()))?;
    let mut tail = vec![0u8; CAPTURE_PREVIEW_TAIL_BYTES];
    file.read_exact(&mut tail)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let omitted = bytes.saturating_sub(preview_capacity as u64);
    Ok(format!(
        "{}\n... [{omitted} bytes omitted from captured stream] ...\n{}",
        String::from_utf8_lossy(&head).trim_end(),
        String::from_utf8_lossy(&tail).trim_start()
    ))
}

fn combined_capture_metrics(stdout_path: &Path, stderr_path: &Path) -> Result<TextMetrics> {
    let stdout = TextMetrics::from_reader(
        File::open(stdout_path)
            .with_context(|| format!("failed to open {}", stdout_path.display()))?,
    )?;
    let stderr = TextMetrics::from_reader(
        File::open(stderr_path)
            .with_context(|| format!("failed to open {}", stderr_path.display()))?,
    )?;
    if stdout.bytes == 0 {
        return Ok(stderr);
    }
    if stderr.bytes == 0 {
        return Ok(stdout);
    }
    Ok(stdout.plus(TextMetrics::from_text("\n")).plus(stderr))
}

fn combine_output(stdout: &str, stderr: &str) -> String {
    if stdout.is_empty() {
        stderr.to_string()
    } else if stderr.is_empty() {
        stdout.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    }
}

fn file_len(path: &Path) -> Result<u64> {
    fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))
        .map(|metadata| metadata.len())
}

fn max_failure_capture_memory_bytes() -> u64 {
    env::var("CX_MAX_FAILURE_CAPTURE_MEMORY_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_FAILURE_CAPTURE_MEMORY_BYTES)
}

fn sanitize_tool_name(tool_name: &str) -> String {
    tool_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::support::test_support::{with_env_vars, with_fake_path, write_executable};

    #[test]
    fn fallback_output_prefers_stdout_when_requested() {
        let captured = CommandOutput::new("one\ntwo\nthree", "err", "one\ntwo\nthree\nerr", 0);
        let fallback = fallback_output(&captured, RunOptions::stdout_only().fallback_window(1, 1));
        assert_eq!(fallback, "one\n... [1 lines omitted] ...\nthree");
    }

    #[test]
    fn failure_stderr_keeps_raw_stderr() {
        let captured = CommandOutput::new(String::new(), "boom\n", "boom\n", 1);
        assert_eq!(failure_stderr(&captured), "boom");
    }

    #[test]
    fn append_failure_hint_uses_hint_when_stdout_empty() {
        assert_eq!(
            append_failure_hint(String::new(), Some("[full output: foo.log]")),
            "[full output: foo.log]"
        );
    }

    #[test]
    fn failure_artifact_hint_marks_the_raw_observation_only_after_storage() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let home_text = home.to_string_lossy().to_string();
        with_env_vars(&[("HOME", Some(home_text.as_str()))], || {
            let mut output = CommandOutput::new("partial\n", "boom\n", "partial\nboom\n", 7);
            assert_eq!(output.observation("fixture").expansion_reason, None);
            let hint = output.failure_artifact_hint("fixture");
            assert!(hint.is_some());
            assert_eq!(
                output.observation("fixture").expansion_reason.as_deref(),
                Some(FAILURE_ARTIFACT_EXPANSION_REASON)
            );
        });
    }

    #[test]
    fn failure_artifact_hint_skips_exactly_empty_streams() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let home_text = home.to_string_lossy().to_string();
        with_env_vars(&[("HOME", Some(home_text.as_str()))], || {
            let mut output = CommandOutput::new("", "", "", 7);
            assert_eq!(output.failure_artifact_hint("fixture"), None);
            assert_eq!(output.observation("fixture").expansion_reason, None);
            assert_eq!(failure_artifact_hint("fixture", "", ""), None);
            assert!(!home.join(".cx/cache/failures/fixture").exists());
        });
    }

    #[test]
    fn failure_artifact_hint_keeps_one_byte_streams() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let home_text = home.to_string_lossy().to_string();
        with_env_vars(&[("HOME", Some(home_text.as_str()))], || {
            let mut stdout_output = CommandOutput::new("x", "", "x", 7);
            assert!(stdout_output.failure_artifact_hint("stdout-byte").is_some());

            let mut stderr_output = CommandOutput::new("", "x", "x", 7);
            assert!(stderr_output.failure_artifact_hint("stderr-byte").is_some());

            assert!(failure_artifact_hint("standalone-byte", "x", "").is_some());
        });
    }

    #[test]
    fn failure_artifact_hint_does_not_claim_failed_storage() {
        let temp = tempfile::tempdir().unwrap();
        let home_file = temp.path().join("home-file");
        fs::write(&home_file, "not a directory").unwrap();
        let home_text = home_file.to_string_lossy().to_string();
        with_env_vars(&[("HOME", Some(home_text.as_str()))], || {
            let mut output = CommandOutput::new("", "boom\n", "boom\n", 7);
            assert_eq!(output.failure_artifact_hint("fixture"), None);
            assert_eq!(output.observation("fixture").expansion_reason, None);
        });
    }

    #[test]
    fn failure_artifact_hint_preserves_a_specific_expansion_reason() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let home_text = home.to_string_lossy().to_string();
        with_env_vars(&[("HOME", Some(home_text.as_str()))], || {
            let mut output = CommandOutput::new("", "boom\n", "boom\n", 7);
            output.raw_observation.expansion_reason = Some("test-result-summary".to_string());
            assert!(output.failure_artifact_hint("fixture").is_some());
            assert_eq!(
                output.observation("fixture").expansion_reason.as_deref(),
                Some("test-result-summary")
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn capture_returns_without_waiting_for_descendant_stdout_to_close() {
        with_fake_path(
            &[(
                "pipe-holder",
                "#!/bin/sh\n(sleep 1) &\nprintf 'done\\n'\nexit 0\n",
            )],
            || {
                let start = Instant::now();
                let output = capture(Command::new("pipe-holder"), "pipe-holder").unwrap();
                assert_eq!(output.exit_code, 0);
                assert!(output.stdout.contains("done"));
                assert!(start.elapsed() < Duration::from_millis(700));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn capture_decodes_invalid_utf8_lossily() {
        with_fake_path(
            &[(
                "invalid-utf8",
                "#!/bin/sh\nprintf '\\377done\\n'\nprintf '\\376err\\n' >&2\nexit 2\n",
            )],
            || {
                let output = capture(Command::new("invalid-utf8"), "invalid-utf8").unwrap();
                assert_eq!(output.exit_code, 2);
                assert!(output.stdout.contains("done"));
                assert!(output.stderr.contains("err"));
                assert!(output.stdout.starts_with(char::REPLACEMENT_CHARACTER));
                assert!(output.stderr.starts_with(char::REPLACEMENT_CHARACTER));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_child_output_survives_an_early_stdin_close() {
        with_fake_path(
            &[(
                "reject-stdin",
                "#!/bin/sh\nexec 0<&-\nprintf 'boom\\n' >&2\nexit 7\n",
            )],
            || {
                let output = capture_with_stdin_timeout(
                    Command::new("reject-stdin"),
                    "reject-stdin",
                    vec![b'x'; 1024 * 1024],
                    Duration::from_secs(2),
                )
                .unwrap();

                assert_eq!(output.exit_code, 7);
                assert_eq!(output.stderr, "boom\n");
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_child_cannot_hide_incomplete_stdin_delivery() {
        with_fake_path(
            &[(
                "accept-without-stdin",
                "#!/bin/sh\nexec 0<&-\nprintf 'ok\\n'\nexit 0\n",
            )],
            || {
                let error = capture_with_stdin_timeout(
                    Command::new("accept-without-stdin"),
                    "accept-without-stdin",
                    vec![b'x'; 1024 * 1024],
                    Duration::from_secs(2),
                )
                .unwrap_err();

                assert!(error.to_string().contains("failed to write"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn large_failures_keep_streaming_metrics_and_a_compressed_recovery_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        let home = temp.path().join("home");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        write_executable(
            &bin,
            "large-failure",
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf 'large failure evidence %04d xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\\n' \"$i\"; i=$((i + 1)); done\nprintf 'terminal failure marker\\n' >&2\nexit 7\n",
        );
        let existing_path = env::var_os("PATH").unwrap_or_default();
        let path = format!("{}:{}", bin.display(), existing_path.to_string_lossy());
        let home_text = home.to_string_lossy().to_string();
        with_env_vars(
            &[
                ("PATH", Some(path.as_str())),
                ("HOME", Some(home_text.as_str())),
                ("CX_MAX_FAILURE_CAPTURE_MEMORY_BYTES", Some("1024")),
                (
                    "CX_FAILURE_ARTIFACT_COMPRESSION_THRESHOLD_BYTES",
                    Some("1024"),
                ),
            ],
            || {
                let mut output = capture(Command::new("large-failure"), "large-failure").unwrap();
                let preview_metrics = TextMetrics::from_text(&output.combined);
                let raw = output.observation("large-failure");
                let hint = output.failure_artifact_hint("large-failure").unwrap();

                assert_eq!(output.exit_code, 7);
                assert!(output.stdout.contains("bytes omitted from captured stream"));
                assert!(output.stderr.contains("terminal failure marker"));
                assert!(raw.metrics.bytes > preview_metrics.bytes);
                assert!(hint.ends_with(".log.gz]"));
                assert_eq!(
                    fs::read_dir(home.join(".cx/cache/failures/large-failure"))
                        .unwrap()
                        .count(),
                    1
                );
            },
        );
    }
}
