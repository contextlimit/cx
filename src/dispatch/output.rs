use std::io::{self, Write};

use anyhow::{Context, Result};

use crate::support::runner::ProxyOutcome;

pub(super) fn emit_proxy_outcome(outcome: &ProxyOutcome) -> Result<i32> {
    let preserve_stream_termination = preserves_stream_termination(outcome);
    let mut stdout = io::stdout().lock();
    write_text_payload(
        &mut stdout,
        &outcome.stdout,
        "stdout",
        preserve_stream_termination,
    )?;
    drop(stdout);

    let mut stderr = io::stderr().lock();
    write_text_payload(
        &mut stderr,
        &outcome.stderr,
        "stderr",
        preserve_stream_termination,
    )?;
    Ok(outcome.exit_code)
}

pub(super) fn emitted_text(outcome: &ProxyOutcome) -> String {
    let preserve_stream_termination = preserves_stream_termination(outcome);
    let mut emitted = String::with_capacity(outcome.stdout.len() + outcome.stderr.len() + 2);
    append_emitted_stream(&mut emitted, &outcome.stdout, preserve_stream_termination);
    append_emitted_stream(&mut emitted, &outcome.stderr, preserve_stream_termination);
    emitted
}

fn preserves_stream_termination(outcome: &ProxyOutcome) -> bool {
    outcome
        .observation
        .as_ref()
        .is_some_and(|observation| observation.uses_preserved_stream_termination())
}

fn append_emitted_stream(emitted: &mut String, text: &str, preserve_stream_termination: bool) {
    emitted.push_str(text);
    if !preserve_stream_termination && !text.is_empty() && !text.ends_with('\n') {
        emitted.push('\n');
    }
}

fn write_text_payload<W: Write>(
    writer: &mut W,
    text: &str,
    stream: &str,
    preserve_stream_termination: bool,
) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    writer
        .write_all(text.as_bytes())
        .with_context(|| format!("failed to write {stream} payload"))?;
    if !preserve_stream_termination && !text.ends_with('\n') {
        writer
            .write_all(b"\n")
            .with_context(|| format!("failed to terminate {stream} payload"))?;
    }
    writer
        .flush()
        .with_context(|| format!("failed to flush {stream} payload"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::insights::OutputObservation;
    use crate::support::runner::ProxyOutcome;

    #[test]
    fn text_payload_can_preserve_or_terminate_native_streams() {
        let mut exact = Vec::new();
        write_text_payload(&mut exact, "native", "test", true).unwrap();
        assert_eq!(exact, b"native");

        let mut terminated = Vec::new();
        write_text_payload(&mut terminated, "summary", "test", false).unwrap();
        assert_eq!(terminated, b"summary\n");
    }

    #[test]
    fn emitted_text_matches_the_selected_stream_policy() {
        let exact = ProxyOutcome::success("native").with_observation(
            OutputObservation::from_text("native", "native").with_preserved_stream_termination(),
        );
        assert_eq!(emitted_text(&exact), "native");

        let summarized = ProxyOutcome::success("summary");
        assert_eq!(emitted_text(&summarized), "summary\n");
    }
}
