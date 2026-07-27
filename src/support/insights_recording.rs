use crate::support::{paths::find_git_root, redaction};
use std::env;

use super::{
    bounded_failure_text, command_root, exit_code_is_actionable_failure, now_ms, ratio, settings,
    ExpansionMetrics, SavedMetrics, TextMetrics, MAX_FAILURE_RESPONSE_CHARS,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputObservation {
    pub source: String,
    pub metrics: TextMetrics,
    pub response: Option<String>,
    pub expansion_reason: Option<String>,
    preserve_stream_termination: bool,
}

impl OutputObservation {
    pub fn from_text(source: impl Into<String>, text: &str) -> Self {
        Self {
            source: source.into(),
            metrics: TextMetrics::from_text(text),
            response: Some(bounded_failure_text(text, MAX_FAILURE_RESPONSE_CHARS)),
            expansion_reason: None,
            preserve_stream_termination: false,
        }
    }

    pub fn from_metrics(source: impl Into<String>, metrics: TextMetrics) -> Self {
        Self {
            source: source.into(),
            metrics,
            response: None,
            expansion_reason: None,
            preserve_stream_termination: false,
        }
    }

    pub fn from_metrics_with_response(
        source: impl Into<String>,
        metrics: TextMetrics,
        response: &str,
    ) -> Self {
        Self {
            source: source.into(),
            metrics,
            response: Some(bounded_failure_text(response, MAX_FAILURE_RESPONSE_CHARS)),
            expansion_reason: None,
            preserve_stream_termination: false,
        }
    }

    pub fn with_expansion_reason(mut self, reason: impl Into<String>) -> Self {
        self.expansion_reason = Some(reason.into());
        self
    }

    pub(crate) fn with_preserved_stream_termination(mut self) -> Self {
        self.preserve_stream_termination = true;
        self
    }

    pub(crate) fn uses_preserved_stream_termination(&self) -> bool {
        self.preserve_stream_termination
    }

    pub(crate) fn use_line_terminated_streams(&mut self) {
        self.preserve_stream_termination = false;
    }
}

#[derive(Debug, Clone)]
pub struct InvocationRecord<'a> {
    pub command: &'a str,
    pub exit_code: i32,
    pub raw: Option<&'a OutputObservation>,
    pub emitted: TextMetrics,
}

#[derive(Debug, Clone)]
pub struct InvocationContext<'a> {
    pub process: &'a str,
    pub command: &'a str,
    pub argv_json: &'a str,
    pub emitted_response: Option<&'a str>,
}

pub(super) struct PreparedInvocation<'a> {
    pub(super) now_ms: u64,
    pub(super) process: &'a str,
    pub(super) command_family: &'a str,
    pub(super) command: &'a str,
    pub(super) argv_json: &'a str,
    pub(super) command_shape: String,
    pub(super) command_shape_hash: String,
    pub(super) source: &'a str,
    pub(super) thread_id: String,
    pub(super) plan_title: String,
    pub(super) plan_project_id: String,
    pub(super) plan_folder_id: String,
    pub(super) cwd: String,
    pub(super) git_root: String,
    pub(super) binary_version: String,
    pub(super) binary_revision: String,
    pub(super) binary_fingerprint: String,
    pub(super) emitted_response_preview: String,
    pub(super) raw_response_preview: String,
    pub(super) exit_code: i32,
    pub(super) raw: TextMetrics,
    pub(super) emitted: TextMetrics,
    pub(super) saved: SavedMetrics,
    pub(super) expanded: ExpansionMetrics,
    pub(super) expansion_reason: String,
    pub(super) savings_ratio: f64,
    pub(super) compression_ratio: f64,
    pub(super) failure_count: i64,
}

impl<'a> PreparedInvocation<'a> {
    pub(super) fn from_record(
        record: &'a InvocationRecord<'a>,
        context: Option<&'a InvocationContext<'a>>,
        settings: settings::InsightSettings,
    ) -> Self {
        let observed_source = record
            .raw
            .map(|raw| raw.source.as_str())
            .unwrap_or("emitted output");
        let raw = record.raw.map(|raw| raw.metrics).unwrap_or(record.emitted);
        let emitted = record.emitted;
        let saved = SavedMetrics::from_raw_and_emitted(raw, emitted);
        let expanded = ExpansionMetrics::from_raw_and_emitted(raw, emitted);
        let expansion_reason = if expanded.any() {
            record
                .raw
                .and_then(|observation| observation.expansion_reason.as_deref())
                .unwrap_or("unclassified-wrapper-expansion")
                .to_string()
        } else {
            String::new()
        };
        let runtime = RuntimeInvocationContext::current();
        let binary = BinaryIdentity::current();
        let compression_ratio = if raw.chars == 0 {
            1.0
        } else {
            emitted.chars as f64 / raw.chars as f64
        };
        let command_shape = if settings.record_command_shape {
            command_shape_from_context(record, context)
        } else {
            String::new()
        };
        let command_shape_hash = if command_shape.is_empty() {
            String::new()
        } else {
            redaction::stable_shape_hash(&command_shape)
        };
        Self {
            now_ms: now_ms(),
            process: context
                .map(|context| context.process)
                .unwrap_or_else(|| command_root(record.command)),
            command_family: record.command,
            command: if settings.record_command_text {
                context
                    .map(|context| context.command)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(record.command)
            } else {
                record.command
            },
            argv_json: if settings.record_command_text {
                context
                    .map(|context| context.argv_json)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("[]")
            } else {
                "[]"
            },
            command_shape,
            command_shape_hash,
            source: if settings.record_sources {
                observed_source
            } else {
                "not recorded"
            },
            thread_id: runtime.thread_id,
            plan_title: runtime.plan_title,
            plan_project_id: runtime.plan_project_id,
            plan_folder_id: runtime.plan_folder_id,
            cwd: runtime.cwd,
            git_root: runtime.git_root,
            binary_version: binary.version,
            binary_revision: binary.revision,
            binary_fingerprint: binary.fingerprint,
            emitted_response_preview: response_preview(
                context.and_then(|context| context.emitted_response),
                settings.record_response_previews,
            ),
            raw_response_preview: response_preview(
                record.raw.and_then(|raw| raw.response.as_deref()),
                settings.record_response_previews,
            ),
            exit_code: record.exit_code,
            raw,
            emitted,
            saved,
            expanded,
            expansion_reason,
            savings_ratio: ratio(saved.chars, raw.chars),
            compression_ratio,
            failure_count: i64::from(exit_code_is_actionable_failure(
                record.command,
                record.exit_code,
            )),
        }
    }
}

pub(super) struct BinaryIdentity {
    pub(super) version: String,
    pub(super) revision: String,
    pub(super) fingerprint: String,
}

impl BinaryIdentity {
    pub(super) fn current() -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();
        let revision = option_env!("CX_BUILD_REVISION")
            .unwrap_or("unknown")
            .to_string();
        let executable = env::current_exe().ok();
        let metadata = executable.as_deref().and_then(|path| path.metadata().ok());
        let fingerprint_source = metadata.map_or_else(
            || format!("{version}:{revision}:unavailable"),
            |metadata| {
                let modified_ns = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default();
                format!("{version}:{revision}:{}:{modified_ns}", metadata.len())
            },
        );
        Self {
            version,
            revision,
            fingerprint: redaction::stable_shape_hash(&fingerprint_source),
        }
    }
}

fn response_preview(response: Option<&str>, enabled: bool) -> String {
    if !enabled {
        return String::new();
    }
    response
        .map(redaction::redact_text)
        .map(|response| bounded_failure_text(&response, MAX_FAILURE_RESPONSE_CHARS))
        .unwrap_or_default()
}

fn command_shape_from_context(
    record: &InvocationRecord<'_>,
    context: Option<&InvocationContext<'_>>,
) -> String {
    let Some(context) = context else {
        return record.command.to_string();
    };
    let args = serde_json::from_str::<Vec<String>>(context.argv_json).unwrap_or_default();
    let args = command_shape_args(&args);
    if args.is_empty() {
        record.command.to_string()
    } else {
        redaction::argument_shape_join(args)
    }
}

fn command_shape_args(args: &[String]) -> &[String] {
    let mut start = usize::from(args.first().is_some_and(|arg| arg == "cx"));
    if args.get(start).is_some_and(|arg| arg == "--") {
        start += 1;
    }
    &args[start..]
}

#[derive(Debug, Clone, Default)]
pub(super) struct RuntimeInvocationContext {
    pub(super) thread_id: String,
    pub(super) plan_title: String,
    pub(super) plan_project_id: String,
    pub(super) plan_folder_id: String,
    pub(super) cwd: String,
    pub(super) git_root: String,
}

impl RuntimeInvocationContext {
    pub(super) fn current() -> Self {
        let cwd_path = env::current_dir().ok();
        let cwd = cwd_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        let git_root = cwd_path
            .as_deref()
            .and_then(find_git_root)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            thread_id: env_text(&["CX_THREAD_ID", "CODEX_THREAD_ID"]),
            plan_title: env_text(&["CX_PLAN_TITLE"]),
            plan_project_id: env_text(&["CX_PLAN_PROJECT_ID"]),
            plan_folder_id: env_text(&["CX_PLAN_FOLDER_ID"]),
            cwd,
            git_root,
        }
    }
}

fn env_text(keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}
