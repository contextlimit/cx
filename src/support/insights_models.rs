use std::io::{self, Read};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextMetrics {
    pub bytes: u64,
    pub chars: u64,
    pub lines: u64,
    pub tokens: u64,
}

impl TextMetrics {
    pub fn from_text(text: &str) -> Self {
        Self {
            bytes: text.len() as u64,
            chars: text.chars().count() as u64,
            lines: count_lines(text),
            tokens: estimate_tokens(text),
        }
    }

    pub fn from_reader<R: Read>(mut reader: R) -> io::Result<Self> {
        let mut accumulator = StreamingTextMetrics::default();
        let mut carry = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            accumulator.observe_bytes(&chunk[..read]);
            let mut decoded = Vec::with_capacity(carry.len() + read);
            decoded.extend_from_slice(&carry);
            decoded.extend_from_slice(&chunk[..read]);
            carry = accumulator.observe_utf8(&decoded, false);
        }
        if !carry.is_empty() {
            accumulator.observe_utf8(&carry, true);
        }
        Ok(accumulator.finish())
    }

    pub fn plus(self, other: Self) -> Self {
        Self {
            bytes: self.bytes + other.bytes,
            chars: self.chars + other.chars,
            lines: self.lines + other.lines,
            tokens: self.tokens + other.tokens,
        }
    }
}

#[derive(Default)]
struct StreamingTextMetrics {
    bytes: u64,
    chars: u64,
    newline_count: u64,
    tokens: u64,
    word_chars: u64,
    has_bytes: bool,
    ends_with_newline: bool,
}

impl StreamingTextMetrics {
    fn observe_bytes(&mut self, bytes: &[u8]) {
        self.bytes += bytes.len() as u64;
        self.newline_count += bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
        if let Some(last) = bytes.last() {
            self.has_bytes = true;
            self.ends_with_newline = *last == b'\n';
        }
    }

    fn observe_utf8(&mut self, bytes: &[u8], eof: bool) -> Vec<u8> {
        let mut cursor = 0usize;
        while cursor < bytes.len() {
            match std::str::from_utf8(&bytes[cursor..]) {
                Ok(text) => {
                    self.observe_text(text);
                    return Vec::new();
                }
                Err(error) => {
                    let valid_end = cursor + error.valid_up_to();
                    if valid_end > cursor {
                        let valid = std::str::from_utf8(&bytes[cursor..valid_end])
                            .expect("UTF-8 validator supplied a valid prefix");
                        self.observe_text(valid);
                    }
                    cursor = valid_end;
                    if let Some(error_len) = error.error_len() {
                        self.observe_text("\u{fffd}");
                        cursor = cursor.saturating_add(error_len);
                    } else if eof {
                        self.observe_text("\u{fffd}");
                        return Vec::new();
                    } else {
                        return bytes[cursor..].to_vec();
                    }
                }
            }
        }
        Vec::new()
    }

    fn observe_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.chars += 1;
            if ch.is_alphanumeric() || ch == '_' {
                self.word_chars += 1;
            } else {
                self.tokens += flush_word_tokens(self.word_chars);
                self.word_chars = 0;
                if !ch.is_whitespace() {
                    self.tokens += 1;
                }
            }
        }
    }

    fn finish(mut self) -> TextMetrics {
        self.tokens += flush_word_tokens(self.word_chars);
        TextMetrics {
            bytes: self.bytes,
            chars: self.chars,
            lines: if !self.has_bytes {
                0
            } else if self.ends_with_newline {
                self.newline_count
            } else {
                self.newline_count + 1
            },
            tokens: self.tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandReportRecord<'a> {
    pub command: &'a str,
    pub command_family: &'a str,
    pub command_shape: &'a str,
    pub command_shape_hash: &'a str,
    pub issue_kind: &'a str,
    pub note: &'a str,
}

pub const GENERIC_COMMAND_REPORT_NOTE: &str =
    "Operator observed incorrect, misleading, incomplete, over-filtered, or suspicious CX output.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReportReceipt {
    pub recorded: bool,
    pub id: u64,
    pub occurred_at_ms: u64,
    pub command_family: String,
    pub total_reports: u64,
    pub family_reports: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReportInsight {
    pub id: u64,
    pub occurred_at_ms: u64,
    pub command_root: String,
    pub command: String,
    pub command_family: String,
    pub issue_kind: String,
    pub note: String,
    pub status: String,
    pub resolution_note: String,
    pub resolution_revision: String,
    pub denial_reason: String,
    pub related_report_id: Option<u64>,
    pub status_updated_at_ms: u64,
    pub invocation_id: Option<u64>,
    pub cwd: String,
    pub git_root: String,
    pub thread_id: String,
    pub command_shape: String,
    pub command_shape_hash: String,
    pub evidence_kind: String,
    pub cx_response: String,
    pub native_response: String,
    pub artifact_ref: String,
    pub binary_version: String,
    pub binary_revision: String,
    pub binary_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReportTotalInsight {
    pub command: String,
    pub reports: u64,
    pub open: u64,
    pub resolved: u64,
    pub native_parity: u64,
    pub not_reproducible: u64,
    pub denied: u64,
    pub other: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandReportStatus {
    Open,
    Resolved,
    NativeParity,
    NotReproducible,
    Denied,
}

impl CommandReportStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::NativeParity => "native_parity",
            Self::NotReproducible => "not_reproducible",
            Self::Denied => "denied",
        }
    }
}

impl std::fmt::Display for CommandReportStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandReportDenialReason {
    Duplicate,
    InsufficientEvidence,
    Invalid,
    Obsolete,
    Unsupported,
    LowValue,
}

impl CommandReportDenialReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::InsufficientEvidence => "insufficient_evidence",
            Self::Invalid => "invalid",
            Self::Obsolete => "obsolete",
            Self::Unsupported => "unsupported",
            Self::LowValue => "low_value",
        }
    }
}

impl std::fmt::Display for CommandReportDenialReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct CommandReportDispositionRecord<'a> {
    pub report_id: u64,
    pub status: CommandReportStatus,
    pub denial_reason: Option<CommandReportDenialReason>,
    pub related_report_id: Option<u64>,
    pub note: &'a str,
    pub revision: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReportDispositionReceipt {
    pub report_id: u64,
    pub command_family: String,
    pub status: CommandReportStatus,
    pub denial_reason: Option<CommandReportDenialReason>,
    pub related_report_id: Option<u64>,
    pub note: String,
    pub revision: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommandReportStatusSummary {
    pub total: u64,
    pub open: u64,
    pub resolved: u64,
    pub native_parity: u64,
    pub not_reproducible: u64,
    pub denied: u64,
    pub other: u64,
}

impl CommandReportStatusSummary {
    pub const fn closed(self) -> u64 {
        self.resolved + self.native_parity + self.not_reproducible + self.denied
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommandReportDenialReasonSummary {
    pub total: u64,
    pub duplicate: u64,
    pub insufficient_evidence: u64,
    pub invalid: u64,
    pub obsolete: u64,
    pub unsupported: u64,
    pub low_value: u64,
    pub other: u64,
}

impl CommandReportDenialReasonSummary {
    pub fn record(&mut self, reason: CommandReportDenialReason) {
        self.total += 1;
        match reason {
            CommandReportDenialReason::Duplicate => self.duplicate += 1,
            CommandReportDenialReason::InsufficientEvidence => self.insufficient_evidence += 1,
            CommandReportDenialReason::Invalid => self.invalid += 1,
            CommandReportDenialReason::Obsolete => self.obsolete += 1,
            CommandReportDenialReason::Unsupported => self.unsupported += 1,
            CommandReportDenialReason::LowValue => self.low_value += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReportTriageProposal {
    pub report_id: u64,
    pub denial_reason: CommandReportDenialReason,
    pub related_report_id: Option<u64>,
    pub policy_rule_id: String,
    pub command_root: String,
    pub command_family: String,
    pub command: String,
    pub evidence_kind: String,
    pub cx_response_recorded: bool,
    pub native_response_recorded: bool,
    pub invocation_recorded: bool,
    pub artifact_recorded: bool,
    pub actionable_evidence_recorded: bool,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReportTriageReceipt {
    pub applied: bool,
    pub current: CommandReportStatusSummary,
    pub projected: CommandReportStatusSummary,
    pub final_status: CommandReportStatusSummary,
    pub denial_reasons: CommandReportDenialReasonSummary,
    pub proposals: Vec<CommandReportTriageProposal>,
    pub applied_count: u64,
    pub skipped_count: u64,
}

#[derive(Debug, Clone)]
pub struct FailureDetailRecord<'a> {
    pub command_family: &'a str,
    pub command_line: &'a str,
    pub exit_code: i32,
    pub cx_response: &'a str,
    pub raw_source: Option<&'a str>,
    pub raw_response: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureDetailInsight {
    pub id: u64,
    pub invocation_id: Option<u64>,
    pub occurred_at_ms: u64,
    pub command_family: String,
    pub command_line: String,
    pub exit_code: i32,
    pub cx_response: String,
    pub raw_source: String,
    pub raw_response: String,
    pub artifact_ref: String,
}

#[derive(Debug, Clone)]
pub struct FailureArtifactRecord<'a> {
    pub display_path: &'a str,
    pub tool_name: &'a str,
    pub compression: &'a str,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub original_bytes: u64,
    pub stored_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureArtifactInsight {
    pub id: u64,
    pub created_at_ms: u64,
    pub display_path: String,
    pub tool_name: String,
    pub compression: String,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub original_bytes: u64,
    pub stored_bytes: u64,
    pub invocation_id: Option<u64>,
    pub report_id: Option<u64>,
    pub exit_code: i32,
    pub binary_revision: String,
    pub binary_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTotalSort {
    Tokens,
    Chars,
    Lines,
    Invocations,
    Failures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLevel {
    Command,
    Root,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavingsSort {
    Tokens,
    Chars,
    Lines,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommandFilter<'a> {
    pub command_root: Option<&'a str>,
    pub command: Option<&'a str>,
}

impl CommandFilter<'_> {
    pub fn is_empty(self) -> bool {
        self.command_root.is_none() && self.command.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandTotalInsight {
    pub command: String,
    pub invocations: u64,
    pub failures: u64,
    pub expansions: u64,
    pub raw: TextMetrics,
    pub emitted: TextMetrics,
    pub saved: TextMetrics,
    pub expanded: TextMetrics,
    pub best_saved_chars: u64,
    pub best_saved_tokens: u64,
    pub best_expanded_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FailureCoverageInsight {
    pub command: String,
    pub failed_invocations: u64,
    pub detail_rows: u64,
    pub linked_detail_rows: u64,
    pub linked_invocations: u64,
    pub orphan_detail_rows: u64,
    pub unknown_invocations: u64,
    pub output_bearing_detail_rows: u64,
    pub silent_detail_rows: u64,
    pub artifact_linked_detail_rows: u64,
    pub output_gap_detail_rows: u64,
    pub response_evidence_available: bool,
    pub artifact_reference_available: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvocationInsight {
    pub id: u64,
    pub occurred_at_ms: u64,
    pub process: String,
    pub command_family: String,
    pub command: String,
    pub argv_json: String,
    pub command_shape: String,
    pub command_shape_hash: String,
    pub source: String,
    pub thread_id: String,
    pub plan_title: String,
    pub plan_project_id: String,
    pub plan_folder_id: String,
    pub cwd: String,
    pub git_root: String,
    pub binary_version: String,
    pub binary_revision: String,
    pub binary_fingerprint: String,
    pub emitted_response_preview: String,
    pub raw_response_preview: String,
    pub exit_code: i32,
    pub raw: TextMetrics,
    pub emitted: TextMetrics,
    pub saved: TextMetrics,
    pub expanded: TextMetrics,
    pub expansion_reason: String,
    pub savings_ratio: f64,
    pub compression_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyInsight {
    pub day: String,
    pub invocations: u64,
    pub failures: u64,
    pub expansions: u64,
    pub saved: TextMetrics,
    pub expanded: TextMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OverallInsight {
    pub invocations: u64,
    pub failures: u64,
    pub expansions: u64,
    pub raw: TextMetrics,
    pub emitted: TextMetrics,
    pub saved: TextMetrics,
    pub expanded: TextMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SavingsDistributionInsight {
    pub invocations: u64,
    pub saving_invocations: u64,
    pub total_saved_tokens: u64,
    pub all_p50_saved_tokens: u64,
    pub all_p95_saved_tokens: u64,
    pub all_p99_saved_tokens: u64,
    pub saving_p50_saved_tokens: u64,
    pub saving_p95_saved_tokens: u64,
    pub saving_p99_saved_tokens: u64,
    pub largest_saved_tokens: u64,
    pub top_ten_saved_tokens: u64,
}

impl SavingsDistributionInsight {
    pub fn saving_invocation_rate(self) -> f64 {
        ratio(self.saving_invocations, self.invocations)
    }

    pub fn largest_share(self) -> f64 {
        ratio(self.largest_saved_tokens, self.total_saved_tokens)
    }

    pub fn top_ten_share(self) -> f64 {
        ratio(self.top_ten_saved_tokens, self.total_saved_tokens)
    }

    pub fn saved_tokens_excluding_largest(self) -> u64 {
        self.total_saved_tokens
            .saturating_sub(self.largest_saved_tokens)
    }

    pub fn saved_tokens_excluding_top_ten(self) -> u64 {
        self.total_saved_tokens
            .saturating_sub(self.top_ten_saved_tokens)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SavedMetrics {
    pub(super) bytes: u64,
    pub(super) chars: u64,
    pub(super) lines: u64,
    pub(super) tokens: u64,
}

impl SavedMetrics {
    pub(super) fn from_raw_and_emitted(raw: TextMetrics, emitted: TextMetrics) -> Self {
        Self {
            bytes: raw.bytes.saturating_sub(emitted.bytes),
            chars: raw.chars.saturating_sub(emitted.chars),
            lines: raw.lines.saturating_sub(emitted.lines),
            tokens: raw.tokens.saturating_sub(emitted.tokens),
        }
    }

    #[cfg(test)]
    pub(super) fn as_text_metrics(self) -> TextMetrics {
        TextMetrics {
            bytes: self.bytes,
            chars: self.chars,
            lines: self.lines,
            tokens: self.tokens,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ExpansionMetrics {
    pub(super) bytes: u64,
    pub(super) chars: u64,
    pub(super) lines: u64,
    pub(super) tokens: u64,
}

impl ExpansionMetrics {
    pub(super) fn from_raw_and_emitted(raw: TextMetrics, emitted: TextMetrics) -> Self {
        Self {
            bytes: emitted.bytes.saturating_sub(raw.bytes),
            chars: emitted.chars.saturating_sub(raw.chars),
            lines: emitted.lines.saturating_sub(raw.lines),
            tokens: emitted.tokens.saturating_sub(raw.tokens),
        }
    }

    pub(super) fn any(self) -> bool {
        self.bytes > 0 || self.chars > 0 || self.lines > 0 || self.tokens > 0
    }

    #[cfg(test)]
    pub(super) fn as_text_metrics(self) -> TextMetrics {
        TextMetrics {
            bytes: self.bytes,
            chars: self.chars,
            lines: self.lines,
            tokens: self.tokens,
        }
    }
}

pub(super) fn ratio(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn count_lines(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    let newline_count = text.bytes().filter(|byte| *byte == b'\n').count() as u64;
    if text.ends_with('\n') {
        newline_count
    } else {
        newline_count + 1
    }
}

fn estimate_tokens(text: &str) -> u64 {
    let mut tokens = 0u64;
    let mut word_chars = 0u64;
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word_chars += 1;
        } else {
            tokens += flush_word_tokens(word_chars);
            word_chars = 0;
            if !ch.is_whitespace() {
                tokens += 1;
            }
        }
    }
    tokens + flush_word_tokens(word_chars)
}

fn flush_word_tokens(word_chars: u64) -> u64 {
    if word_chars == 0 {
        0
    } else {
        word_chars.div_ceil(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_metrics_count_chars_lines_and_estimated_tokens() {
        let metrics = TextMetrics::from_text("alpha beta\n{}\n");
        assert_eq!(metrics.bytes, 14);
        assert_eq!(metrics.chars, 14);
        assert_eq!(metrics.lines, 2);
        assert_eq!(metrics.tokens, 5);
    }

    #[test]
    fn streaming_text_metrics_match_lossy_text_metrics_across_chunk_boundaries() {
        let mut input = vec![b'a'; 64 * 1024 - 1];
        input.extend_from_slice("é beta\n{}\n".as_bytes());
        input.push(0xff);
        let expected_text = TextMetrics::from_text(&String::from_utf8_lossy(&input));
        let expected = TextMetrics {
            bytes: input.len() as u64,
            ..expected_text
        };

        assert_eq!(
            TextMetrics::from_reader(input.as_slice()).unwrap(),
            expected
        );
    }

    #[test]
    fn saved_metrics_saturate_when_output_expands() {
        let saved = SavedMetrics::from_raw_and_emitted(
            TextMetrics::from_text("raw"),
            TextMetrics::from_text("expanded output"),
        );
        assert_eq!(saved.as_text_metrics(), TextMetrics::default());
    }

    #[test]
    fn expansion_metrics_capture_only_emitted_growth() {
        let expanded = ExpansionMetrics::from_raw_and_emitted(
            TextMetrics::from_text("raw"),
            TextMetrics::from_text("expanded output"),
        );
        assert!(expanded.any());
        assert_eq!(expanded.chars, 12);
        assert_eq!(expanded.lines, 0);
        assert_eq!(expanded.tokens, 3);

        let reduced = ExpansionMetrics::from_raw_and_emitted(
            TextMetrics::from_text("expanded output"),
            TextMetrics::from_text("raw"),
        );
        assert!(!reduced.any());
        assert_eq!(reduced.as_text_metrics(), TextMetrics::default());
    }
}
