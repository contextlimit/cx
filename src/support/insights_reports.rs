use anyhow::{bail, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, TransactionBehavior};

use super::queries::command_report_root_query_expression;
use super::query_api::{bounded_limit, command_root};
use super::recording::{BinaryIdentity, PreparedInvocation, RuntimeInvocationContext};
use super::settings::PENDING_REPORT_EVIDENCE_AT_MS;
use super::store::{column_exists, open_readonly_database, table_exists};
use super::{
    now_ms, CommandFilter, CommandLevel, CommandReportDenialReason,
    CommandReportDenialReasonSummary, CommandReportDispositionReceipt,
    CommandReportDispositionRecord, CommandReportInsight, CommandReportReceipt,
    CommandReportRecord, CommandReportStatus, CommandReportStatusSummary,
    CommandReportTotalInsight,
};

const DISPOSITION_TABLE: &str = "command_report_dispositions";
const REPORT_EVIDENCE_LOOKBACK_MS: u64 = 10 * 60 * 1_000;

pub(super) fn create_command_reports_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS command_reports (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at_ms INTEGER NOT NULL,
            command TEXT NOT NULL,
            command_root TEXT NOT NULL DEFAULT '',
            command_family TEXT NOT NULL,
            issue_kind TEXT NOT NULL,
            note TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_command_reports_time
            ON command_reports(occurred_at_ms);
        CREATE INDEX IF NOT EXISTS idx_command_reports_family_time
            ON command_reports(command_family, occurred_at_ms);

        CREATE TABLE IF NOT EXISTS command_report_dispositions (
            report_id INTEGER PRIMARY KEY,
            status TEXT NOT NULL CHECK (
                status IN ('open', 'resolved', 'native_parity', 'not_reproducible', 'denied')
            ),
            note TEXT NOT NULL,
            revision TEXT NOT NULL DEFAULT '',
            denial_reason TEXT NOT NULL DEFAULT '',
            related_report_id INTEGER,
            updated_at_ms INTEGER NOT NULL,
            FOREIGN KEY(report_id) REFERENCES command_reports(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_command_report_dispositions_status_time
            ON command_report_dispositions(status, updated_at_ms);

        CREATE TABLE IF NOT EXISTS command_report_evidence (
            report_id INTEGER PRIMARY KEY,
            invocation_id INTEGER,
            captured_at_ms INTEGER NOT NULL,
            cwd TEXT NOT NULL DEFAULT '',
            git_root TEXT NOT NULL DEFAULT '',
            thread_id TEXT NOT NULL DEFAULT '',
            command_shape TEXT NOT NULL DEFAULT '',
            command_shape_hash TEXT NOT NULL DEFAULT '',
            evidence_kind TEXT NOT NULL DEFAULT 'legacy',
            cx_response TEXT NOT NULL DEFAULT '',
            native_response TEXT NOT NULL DEFAULT '',
            artifact_ref TEXT NOT NULL DEFAULT '',
            binary_version TEXT NOT NULL DEFAULT '',
            binary_revision TEXT NOT NULL DEFAULT '',
            binary_fingerprint TEXT NOT NULL DEFAULT '',
            FOREIGN KEY(report_id) REFERENCES command_reports(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_command_report_evidence_invocation
            ON command_report_evidence(invocation_id);
        ",
    )?;
    Ok(())
}

pub(super) fn insert_command_report(
    connection: &mut Connection,
    record: &CommandReportRecord<'_>,
    runtime: &RuntimeInvocationContext,
) -> Result<CommandReportReceipt> {
    let occurred_at_ms = now_ms();
    let root = command_root(record.command_family);
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "
        INSERT INTO command_reports (
            occurred_at_ms, command, command_root, command_family, issue_kind, note
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        params![
            occurred_at_ms,
            record.command,
            root,
            record.command_family,
            record.issue_kind,
            record.note,
        ],
    )?;
    let id = transaction.last_insert_rowid() as u64;
    let needs_late_evidence =
        insert_report_evidence(&transaction, id, occurred_at_ms, record, runtime)?;
    if needs_late_evidence {
        set_pending_report_evidence_at_ms(&transaction, occurred_at_ms)?;
    }
    let total_reports = count_command_reports(&transaction, None)?;
    let family_reports = count_command_reports(&transaction, Some(record.command_family))?;
    transaction.commit()?;
    Ok(CommandReportReceipt {
        recorded: true,
        id,
        occurred_at_ms,
        command_family: record.command_family.to_string(),
        total_reports,
        family_reports,
    })
}

pub(super) fn backfill_recent_command_report_evidence(
    connection: &mut Connection,
    invocation_id: u64,
    prepared: &PreparedInvocation<'_>,
) -> Result<u64> {
    if !table_exists(connection, "command_reports")?
        || !table_exists(connection, "command_report_evidence")?
        || !table_exists(connection, "command_invocations")?
    {
        return Ok(0);
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let report_ids = late_report_ids(&transaction, prepared)?;
    if report_ids.is_empty() {
        refresh_pending_report_evidence_at_ms(&transaction, prepared.now_ms)?;
        transaction.commit()?;
        return Ok(0);
    }

    let mut evidence = ReportEvidence::from_prepared_invocation(invocation_id, prepared);
    evidence.attach_failure_detail(&transaction)?;
    let invocation = LateInvocationEvidence {
        invocation_id,
        evidence,
        artifact_ref: latest_artifact_ref(&transaction, Some(invocation_id))?,
    };
    let updated = write_late_report_evidence(&transaction, &report_ids, &invocation)?;
    refresh_pending_report_evidence_at_ms(&transaction, prepared.now_ms)?;
    transaction.commit()?;
    Ok(updated)
}

pub(super) fn pending_report_evidence_may_exist(pending_at_ms: u64, now_ms: u64) -> bool {
    pending_at_ms > 0 && now_ms.saturating_sub(pending_at_ms) <= REPORT_EVIDENCE_LOOKBACK_MS
}

fn set_pending_report_evidence_at_ms(
    transaction: &rusqlite::Transaction<'_>,
    occurred_at_ms: u64,
) -> Result<()> {
    transaction.execute(
        "
        INSERT INTO settings (key, value, updated_at_ms)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![
            PENDING_REPORT_EVIDENCE_AT_MS,
            occurred_at_ms.to_string(),
            occurred_at_ms,
        ],
    )?;
    Ok(())
}

fn refresh_pending_report_evidence_at_ms(
    transaction: &rusqlite::Transaction<'_>,
    now_ms: u64,
) -> Result<()> {
    let start_ms = now_ms.saturating_sub(REPORT_EVIDENCE_LOOKBACK_MS);
    let pending_at_ms = transaction.query_row(
        "
        SELECT MAX(r.occurred_at_ms)
        FROM command_reports AS r
        JOIN command_report_evidence AS e ON e.report_id = r.id
        LEFT JOIN command_report_dispositions AS d ON d.report_id = r.id
        WHERE COALESCE(d.status, 'open') = 'open'
          AND e.invocation_id IS NULL
          AND e.evidence_kind = 'no-match'
          AND r.occurred_at_ms BETWEEN ?1 AND ?2
        ",
        params![start_ms, now_ms],
        |row| row.get::<_, Option<u64>>(0),
    )?;
    set_pending_report_evidence_at_ms(transaction, pending_at_ms.unwrap_or_default())
}

struct LateInvocationEvidence {
    invocation_id: u64,
    evidence: ReportEvidence,
    artifact_ref: String,
}

fn late_report_ids(
    transaction: &rusqlite::Transaction<'_>,
    prepared: &PreparedInvocation<'_>,
) -> Result<Vec<u64>> {
    let start_ms = prepared.now_ms.saturating_sub(REPORT_EVIDENCE_LOOKBACK_MS);
    let mut statement = transaction.prepare(
        "
        SELECT r.id
        FROM command_reports AS r
        JOIN command_report_evidence AS e ON e.report_id = r.id
        LEFT JOIN command_report_dispositions AS d ON d.report_id = r.id
        WHERE COALESCE(d.status, 'open') = 'open'
          AND r.command_family = ?1
          AND r.command = ?2
          AND e.invocation_id IS NULL
          AND e.evidence_kind = 'no-match'
          AND e.cwd = ?3
          AND e.git_root = ?4
          AND e.thread_id = ?5
          AND e.command_shape = ?6
          AND e.command_shape_hash = ?7
          AND r.occurred_at_ms BETWEEN ?8 AND ?9
        ORDER BY r.occurred_at_ms DESC, r.id DESC
        LIMIT 16
        ",
    )?;
    let rows = statement.query_map(
        params![
            prepared.command_family,
            prepared.command,
            prepared.cwd,
            prepared.git_root,
            prepared.thread_id,
            prepared.command_shape,
            prepared.command_shape_hash,
            start_ms,
            prepared.now_ms,
        ],
        |row| row.get::<_, u64>(0),
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn write_late_report_evidence(
    transaction: &rusqlite::Transaction<'_>,
    report_ids: &[u64],
    invocation: &LateInvocationEvidence,
) -> Result<u64> {
    let evidence_kind = invocation
        .evidence
        .evidence_kind
        .strip_prefix("exact-command:")
        .map(|source| format!("exact-command:late-{source}"))
        .unwrap_or_else(|| "exact-command:late-metadata-only".to_string());
    let mut updated = 0_u64;
    for report_id in report_ids {
        updated +=
            update_late_report_evidence(transaction, *report_id, invocation, &evidence_kind)?;
    }
    if !invocation.artifact_ref.is_empty() {
        transaction.execute(
            "
            UPDATE failure_artifacts
            SET report_id = COALESCE(report_id, ?1)
            WHERE display_path = ?2
            ",
            params![report_ids[0], invocation.artifact_ref],
        )?;
    }
    Ok(updated)
}

fn update_late_report_evidence(
    transaction: &rusqlite::Transaction<'_>,
    report_id: u64,
    invocation: &LateInvocationEvidence,
    evidence_kind: &str,
) -> Result<u64> {
    let updated = transaction.execute(
        "
        UPDATE command_report_evidence
        SET invocation_id = ?2,
            cwd = ?3,
            git_root = ?4,
            thread_id = ?5,
            command_shape = ?6,
            command_shape_hash = ?7,
            evidence_kind = ?8,
            cx_response = ?9,
            native_response = ?10,
            artifact_ref = ?11,
            binary_version = ?12,
            binary_revision = ?13,
            binary_fingerprint = ?14
        WHERE report_id = ?1
          AND invocation_id IS NULL
          AND evidence_kind = 'no-match'
        ",
        params![
            report_id,
            invocation.invocation_id,
            invocation.evidence.cwd,
            invocation.evidence.git_root,
            invocation.evidence.thread_id,
            invocation.evidence.command_shape,
            invocation.evidence.command_shape_hash,
            evidence_kind,
            invocation.evidence.cx_response,
            invocation.evidence.native_response,
            invocation.artifact_ref,
            invocation.evidence.binary_version,
            invocation.evidence.binary_revision,
            invocation.evidence.binary_fingerprint,
        ],
    )?;
    Ok(updated as u64)
}

fn insert_report_evidence(
    transaction: &rusqlite::Transaction<'_>,
    report_id: u64,
    captured_at_ms: u64,
    record: &CommandReportRecord<'_>,
    runtime: &RuntimeInvocationContext,
) -> Result<bool> {
    let evidence = load_report_evidence(transaction, captured_at_ms, record, runtime)?;
    let needs_late_evidence = evidence.invocation_id.is_none();
    let artifact_ref = latest_artifact_ref(transaction, evidence.invocation_id)?;
    transaction.execute(
        "
        INSERT INTO command_report_evidence (
            report_id, invocation_id, captured_at_ms, cwd, git_root, thread_id,
            command_shape, command_shape_hash, evidence_kind, cx_response,
            native_response, artifact_ref, binary_version, binary_revision,
            binary_fingerprint
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
        )
        ",
        params![
            report_id,
            evidence.invocation_id,
            captured_at_ms,
            evidence.cwd,
            evidence.git_root,
            evidence.thread_id,
            evidence.command_shape,
            evidence.command_shape_hash,
            evidence.evidence_kind,
            evidence.cx_response,
            evidence.native_response,
            artifact_ref,
            evidence.binary_version,
            evidence.binary_revision,
            evidence.binary_fingerprint,
        ],
    )?;
    if !artifact_ref.is_empty() {
        transaction.execute(
            "UPDATE failure_artifacts SET report_id = ?1 WHERE display_path = ?2",
            params![report_id, artifact_ref],
        )?;
    }
    Ok(needs_late_evidence)
}

fn load_report_evidence(
    transaction: &rusqlite::Transaction<'_>,
    captured_at_ms: u64,
    record: &CommandReportRecord<'_>,
    runtime: &RuntimeInvocationContext,
) -> Result<ReportEvidence> {
    let start_ms = captured_at_ms.saturating_sub(REPORT_EVIDENCE_LOOKBACK_MS);
    let mut evidence = if let Some(exact) =
        load_exact_command_evidence(transaction, record, runtime, start_ms, captured_at_ms)?
    {
        exact
    } else if let Some(unique) =
        load_unique_shape_evidence(transaction, record, runtime, start_ms, captured_at_ms)?
    {
        unique
    } else {
        ReportEvidence::fallback(runtime, record)
    };
    evidence.attach_failure_detail(transaction)?;
    Ok(evidence)
}

fn load_exact_command_evidence(
    transaction: &rusqlite::Transaction<'_>,
    record: &CommandReportRecord<'_>,
    runtime: &RuntimeInvocationContext,
    start_ms: u64,
    captured_at_ms: u64,
) -> Result<Option<ReportEvidence>> {
    transaction
        .query_row(
            "
            SELECT id, cwd, git_root, thread_id, command_shape, command_shape_hash,
                   emitted_response_preview, raw_response_preview,
                   binary_version, binary_revision, binary_fingerprint
            FROM command_invocations
            WHERE command_family = ?1
              AND command = ?2
              AND (?3 = '' OR command_shape = ?3)
              AND (?4 = '' OR command_shape_hash = ?4)
              AND (?5 = '' OR cwd = ?5)
              AND (?6 = '' OR thread_id = ?6)
              AND occurred_at_ms BETWEEN ?7 AND ?8
            ORDER BY occurred_at_ms DESC, id DESC
            LIMIT 1
            ",
            params![
                record.command_family,
                record.command,
                record.command_shape,
                record.command_shape_hash,
                runtime.cwd,
                runtime.thread_id,
                start_ms,
                captured_at_ms,
            ],
            |row| ReportEvidence::from_invocation_row(row, EvidenceMatch::ExactCommand),
        )
        .optional()
        .map_err(Into::into)
}

fn load_unique_shape_evidence(
    transaction: &rusqlite::Transaction<'_>,
    record: &CommandReportRecord<'_>,
    runtime: &RuntimeInvocationContext,
    start_ms: u64,
    captured_at_ms: u64,
) -> Result<Option<ReportEvidence>> {
    if record.command_shape.is_empty() || record.command_shape_hash.is_empty() {
        return Ok(None);
    }
    let mut statement = transaction.prepare(
        "
        SELECT id, cwd, git_root, thread_id, command_shape, command_shape_hash,
               emitted_response_preview, raw_response_preview,
               binary_version, binary_revision, binary_fingerprint
        FROM command_invocations
        WHERE command_family = ?1
          AND command_shape = ?2
          AND command_shape_hash = ?3
          AND (argv_json = '[]' OR argv_json = '')
          AND (?4 = '' OR cwd = ?4)
          AND (?5 = '' OR thread_id = ?5)
          AND occurred_at_ms BETWEEN ?6 AND ?7
        ORDER BY occurred_at_ms DESC, id DESC
        LIMIT 2
        ",
    )?;
    let rows = statement.query_map(
        params![
            record.command_family,
            record.command_shape,
            record.command_shape_hash,
            runtime.cwd,
            runtime.thread_id,
            start_ms,
            captured_at_ms,
        ],
        |row| ReportEvidence::from_invocation_row(row, EvidenceMatch::UniqueShape),
    )?;
    let candidates = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((candidates.len() == 1)
        .then(|| candidates.into_iter().next())
        .flatten())
}

fn latest_artifact_ref(
    transaction: &rusqlite::Transaction<'_>,
    invocation_id: Option<u64>,
) -> Result<String> {
    let Some(invocation_id) = invocation_id else {
        return Ok(String::new());
    };
    transaction
        .query_row(
            "
        SELECT display_path
        FROM failure_artifacts
        WHERE invocation_id = ?1
        ORDER BY created_at_ms DESC, id DESC
        LIMIT 1
        ",
            [invocation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map(|value| value.unwrap_or_default())
        .map_err(Into::into)
}

struct ReportEvidence {
    invocation_id: Option<u64>,
    match_kind: Option<EvidenceMatch>,
    cwd: String,
    git_root: String,
    thread_id: String,
    command_shape: String,
    command_shape_hash: String,
    evidence_kind: String,
    cx_response: String,
    native_response: String,
    binary_version: String,
    binary_revision: String,
    binary_fingerprint: String,
}

#[derive(Debug, Clone, Copy)]
enum EvidenceMatch {
    ExactCommand,
    UniqueShape,
}

impl ReportEvidence {
    fn from_prepared_invocation(invocation_id: u64, prepared: &PreparedInvocation<'_>) -> Self {
        let has_response = !prepared.emitted_response_preview.is_empty()
            || !prepared.raw_response_preview.is_empty();
        Self {
            invocation_id: Some(invocation_id),
            match_kind: Some(EvidenceMatch::ExactCommand),
            cwd: prepared.cwd.clone(),
            git_root: prepared.git_root.clone(),
            thread_id: prepared.thread_id.clone(),
            command_shape: prepared.command_shape.clone(),
            command_shape_hash: prepared.command_shape_hash.clone(),
            evidence_kind: evidence_kind(EvidenceMatch::ExactCommand, has_response, false),
            cx_response: prepared.emitted_response_preview.clone(),
            native_response: prepared.raw_response_preview.clone(),
            binary_version: prepared.binary_version.clone(),
            binary_revision: prepared.binary_revision.clone(),
            binary_fingerprint: prepared.binary_fingerprint.clone(),
        }
    }

    fn from_invocation_row(
        row: &rusqlite::Row<'_>,
        match_kind: EvidenceMatch,
    ) -> rusqlite::Result<Self> {
        let cx_response: String = row.get(6)?;
        let native_response: String = row.get(7)?;
        Ok(Self {
            invocation_id: Some(row.get(0)?),
            match_kind: Some(match_kind),
            cwd: row.get(1)?,
            git_root: row.get(2)?,
            thread_id: row.get(3)?,
            command_shape: row.get(4)?,
            command_shape_hash: row.get(5)?,
            evidence_kind: evidence_kind(
                match_kind,
                !cx_response.is_empty() || !native_response.is_empty(),
                false,
            ),
            cx_response,
            native_response,
            binary_version: row.get(8)?,
            binary_revision: row.get(9)?,
            binary_fingerprint: row.get(10)?,
        })
    }

    fn fallback(runtime: &RuntimeInvocationContext, record: &CommandReportRecord<'_>) -> Self {
        let binary = BinaryIdentity::current();
        Self {
            invocation_id: None,
            match_kind: None,
            cwd: runtime.cwd.clone(),
            git_root: runtime.git_root.clone(),
            thread_id: runtime.thread_id.clone(),
            command_shape: record.command_shape.to_string(),
            command_shape_hash: record.command_shape_hash.to_string(),
            evidence_kind: "no-match".to_string(),
            cx_response: String::new(),
            native_response: String::new(),
            binary_version: binary.version,
            binary_revision: binary.revision,
            binary_fingerprint: binary.fingerprint,
        }
    }

    fn attach_failure_detail(&mut self, transaction: &rusqlite::Transaction<'_>) -> Result<()> {
        let Some(invocation_id) = self.invocation_id else {
            return Ok(());
        };
        if !column_exists(transaction, "command_failures", "invocation_id")? {
            return Ok(());
        }
        let failure = transaction
            .query_row(
                "
                SELECT cx_response, raw_response
                FROM command_failures
                WHERE invocation_id = ?1
                ORDER BY occurred_at_ms DESC, id DESC
                LIMIT 1
                ",
                [invocation_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((cx_response, native_response)) = failure else {
            return Ok(());
        };
        let mut used_failure_detail = false;
        if self.cx_response.is_empty() && !cx_response.is_empty() {
            self.cx_response = cx_response;
            used_failure_detail = true;
        }
        if self.native_response.is_empty() && !native_response.is_empty() {
            self.native_response = native_response;
            used_failure_detail = true;
        }
        if !used_failure_detail {
            return Ok(());
        }
        let Some(match_kind) = self.match_kind else {
            return Ok(());
        };
        self.evidence_kind = evidence_kind(
            match_kind,
            !self.cx_response.is_empty() || !self.native_response.is_empty(),
            true,
        );
        Ok(())
    }
}

fn evidence_kind(match_kind: EvidenceMatch, has_response: bool, from_failure: bool) -> String {
    let matched_by = match match_kind {
        EvidenceMatch::ExactCommand => "exact-command",
        EvidenceMatch::UniqueShape => "unique-shape",
    };
    let source = if from_failure && has_response {
        "failure-detail"
    } else if has_response {
        "invocation-preview"
    } else {
        "metadata-only"
    };
    format!("{matched_by}:{source}")
}

pub(super) fn upsert_command_report_disposition(
    connection: &mut Connection,
    record: &CommandReportDispositionRecord<'_>,
) -> Result<CommandReportDispositionReceipt> {
    if record.note.trim().is_empty() {
        bail!("report lifecycle updates require a non-empty note");
    }
    validate_disposition(record)?;
    let updated_at_ms = now_ms();
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let command_family = transaction
        .query_row(
            "SELECT command_family FROM command_reports WHERE id = ?1",
            [record.report_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| {
            if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
                anyhow::anyhow!("command-quality report {} was not found", record.report_id)
            } else {
                error.into()
            }
        })?;
    if let Some(related_report_id) = record.related_report_id {
        let related_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM command_reports WHERE id = ?1)",
            [related_report_id],
            |row| row.get(0),
        )?;
        if !related_exists {
            bail!("related command-quality report {related_report_id} was not found");
        }
    }
    transaction.execute(
        "
        INSERT INTO command_report_dispositions (
            report_id, status, note, revision, denial_reason, related_report_id,
            updated_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(report_id) DO UPDATE SET
            status = excluded.status,
            note = excluded.note,
            revision = excluded.revision,
            denial_reason = excluded.denial_reason,
            related_report_id = excluded.related_report_id,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![
            record.report_id,
            record.status.as_str(),
            record.note.trim(),
            record.revision.trim(),
            record
                .denial_reason
                .map(CommandReportDenialReason::as_str)
                .unwrap_or(""),
            record.related_report_id,
            updated_at_ms,
        ],
    )?;
    transaction.commit()?;
    Ok(CommandReportDispositionReceipt {
        report_id: record.report_id,
        command_family,
        status: record.status,
        denial_reason: record.denial_reason,
        related_report_id: record.related_report_id,
        note: record.note.trim().to_string(),
        revision: record.revision.trim().to_string(),
        updated_at_ms,
    })
}

fn validate_disposition(record: &CommandReportDispositionRecord<'_>) -> Result<()> {
    match record.status {
        CommandReportStatus::Denied => {
            let Some(reason) = record.denial_reason else {
                bail!("denied report lifecycle updates require --reason");
            };
            if reason == CommandReportDenialReason::Duplicate && record.related_report_id.is_none()
            {
                bail!("duplicate report denials require --related-report-id");
            }
        }
        _ => {
            if record.denial_reason.is_some() {
                bail!("denial reasons are valid only when report status is denied");
            }
            if record.related_report_id.is_some() {
                bail!("related report ids are valid only when report status is denied");
            }
        }
    }
    if record.related_report_id == Some(record.report_id) {
        bail!("a denied report cannot reference itself as the related report");
    }
    Ok(())
}

fn count_command_reports(connection: &Connection, command_family: Option<&str>) -> Result<u64> {
    let count: i64 = if let Some(command_family) = command_family {
        connection.query_row(
            "SELECT COUNT(*) FROM command_reports WHERE command_family = ?1",
            [command_family],
            |row| row.get(0),
        )?
    } else {
        connection.query_row("SELECT COUNT(*) FROM command_reports", [], |row| row.get(0))?
    };
    Ok(count.max(0) as u64)
}

pub fn recent_command_reports(limit: usize) -> Result<Vec<CommandReportInsight>> {
    recent_command_reports_filtered_by_status(limit, CommandFilter::default(), None)
}

pub fn recent_command_reports_filtered(
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<CommandReportInsight>> {
    recent_command_reports_filtered_by_status(limit, filter, None)
}

pub fn recent_command_reports_filtered_by_status(
    limit: usize,
    filter: CommandFilter<'_>,
    status: Option<CommandReportStatus>,
) -> Result<Vec<CommandReportInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_reports")? {
        return Ok(Vec::new());
    }
    let expressions = ReportDispositionExpressions::load(&connection)?;
    let evidence = ReportEvidenceExpressions::load(&connection)?;
    let filter_sql = command_report_filter_sql(&connection, filter, status, &expressions.status)?;
    let command_root = command_report_root_query_expression(&connection)?;
    let sql = format!(
        "
        SELECT id, occurred_at_ms, {command_root} AS command_root,
               command, command_family, issue_kind, note,
               {} AS status, {} AS resolution_note,
               {} AS resolution_revision, {} AS denial_reason,
               {} AS related_report_id, {} AS status_updated_at_ms,
               {} AS invocation_id, {} AS cwd, {} AS git_root, {} AS thread_id,
               {} AS command_shape, {} AS command_shape_hash,
               {} AS evidence_kind, {} AS cx_response, {} AS native_response,
               {} AS artifact_ref, {} AS binary_version, {} AS binary_revision,
               {} AS binary_fingerprint
        FROM command_reports
        {}
        ORDER BY occurred_at_ms DESC, id DESC
        LIMIT ?
        ",
        expressions.status,
        expressions.note,
        expressions.revision,
        expressions.denial_reason,
        expressions.related_report_id,
        expressions.updated_at_ms,
        evidence.invocation_id,
        evidence.cwd,
        evidence.git_root,
        evidence.thread_id,
        evidence.command_shape,
        evidence.command_shape_hash,
        evidence.evidence_kind,
        evidence.cx_response,
        evidence.native_response,
        evidence.artifact_ref,
        evidence.binary_version,
        evidence.binary_revision,
        evidence.binary_fingerprint,
        filter_sql.where_clause,
    );
    let mut statement = connection.prepare(&sql)?;
    let mut query_params = filter_sql.params;
    query_params.push(bounded_limit(limit).to_string());
    let rows = statement.query_map(params_from_iter(query_params.iter()), |row| {
        Ok(CommandReportInsight {
            id: row.get(0)?,
            occurred_at_ms: row.get(1)?,
            command_root: row.get(2)?,
            command: row.get(3)?,
            command_family: row.get(4)?,
            issue_kind: row.get(5)?,
            note: row.get(6)?,
            status: row.get(7)?,
            resolution_note: row.get(8)?,
            resolution_revision: row.get(9)?,
            denial_reason: row.get(10)?,
            related_report_id: row.get(11)?,
            status_updated_at_ms: row.get(12)?,
            invocation_id: row.get(13)?,
            cwd: row.get(14)?,
            git_root: row.get(15)?,
            thread_id: row.get(16)?,
            command_shape: row.get(17)?,
            command_shape_hash: row.get(18)?,
            evidence_kind: row.get(19)?,
            cx_response: row.get(20)?,
            native_response: row.get(21)?,
            artifact_ref: row.get(22)?,
            binary_version: row.get(23)?,
            binary_revision: row.get(24)?,
            binary_fingerprint: row.get(25)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn command_report_totals_at_level(
    level: CommandLevel,
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<CommandReportTotalInsight>> {
    command_report_totals_at_level_by_status(level, limit, filter, None)
}

pub fn command_report_totals_at_level_by_status(
    level: CommandLevel,
    limit: usize,
    filter: CommandFilter<'_>,
    status: Option<CommandReportStatus>,
) -> Result<Vec<CommandReportTotalInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_reports")? {
        return Ok(Vec::new());
    }
    let expressions = ReportDispositionExpressions::load(&connection)?;
    let command_root = command_report_root_query_expression(&connection)?;
    let group_expression = match level {
        CommandLevel::Command => "command_family".to_string(),
        CommandLevel::Root => command_root,
    };
    let filter_sql = command_report_filter_sql(&connection, filter, status, &expressions.status)?;
    let known_statuses = "'open', 'resolved', 'native_parity', 'not_reproducible', 'denied'";
    let sql = format!(
        "
        SELECT {group_expression} AS command, COUNT(*) AS reports,
               SUM(CASE WHEN {} = 'open' THEN 1 ELSE 0 END) AS open,
               SUM(CASE WHEN {} = 'resolved' THEN 1 ELSE 0 END) AS resolved,
               SUM(CASE WHEN {} = 'native_parity' THEN 1 ELSE 0 END) AS native_parity,
               SUM(CASE WHEN {} = 'not_reproducible' THEN 1 ELSE 0 END) AS not_reproducible,
               SUM(CASE WHEN {} = 'denied' THEN 1 ELSE 0 END) AS denied,
               SUM(CASE WHEN {} NOT IN ({known_statuses}) THEN 1 ELSE 0 END) AS other
        FROM command_reports
        {}
        GROUP BY 1
        ORDER BY reports DESC, command ASC
        LIMIT ?
        ",
        expressions.status,
        expressions.status,
        expressions.status,
        expressions.status,
        expressions.status,
        expressions.status,
        filter_sql.where_clause,
    );
    let mut statement = connection.prepare(&sql)?;
    let mut query_params = filter_sql.params;
    query_params.push(bounded_limit(limit).to_string());
    let rows = statement.query_map(params_from_iter(query_params.iter()), |row| {
        Ok(CommandReportTotalInsight {
            command: row.get(0)?,
            reports: row.get(1)?,
            open: row.get(2)?,
            resolved: row.get(3)?,
            native_parity: row.get(4)?,
            not_reproducible: row.get(5)?,
            denied: row.get(6)?,
            other: row.get(7)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn command_report_status_summary(
    filter: CommandFilter<'_>,
) -> Result<CommandReportStatusSummary> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(CommandReportStatusSummary::default());
    };
    if !table_exists(&connection, "command_reports")? {
        return Ok(CommandReportStatusSummary::default());
    }
    let expressions = ReportDispositionExpressions::load(&connection)?;
    let filter_sql = command_report_filter_sql(&connection, filter, None, &expressions.status)?;
    let known_statuses = "'open', 'resolved', 'native_parity', 'not_reproducible', 'denied'";
    let sql = format!(
        "
        SELECT COUNT(*) AS total,
               COALESCE(SUM(CASE WHEN {} = 'open' THEN 1 ELSE 0 END), 0) AS open,
               COALESCE(SUM(CASE WHEN {} = 'resolved' THEN 1 ELSE 0 END), 0) AS resolved,
               COALESCE(SUM(CASE WHEN {} = 'native_parity' THEN 1 ELSE 0 END), 0) AS native_parity,
               COALESCE(SUM(CASE WHEN {} = 'not_reproducible' THEN 1 ELSE 0 END), 0) AS not_reproducible,
               COALESCE(SUM(CASE WHEN {} = 'denied' THEN 1 ELSE 0 END), 0) AS denied,
               COALESCE(SUM(CASE WHEN {} NOT IN ({known_statuses}) THEN 1 ELSE 0 END), 0) AS other
        FROM command_reports
        {}
        ",
        expressions.status,
        expressions.status,
        expressions.status,
        expressions.status,
        expressions.status,
        expressions.status,
        filter_sql.where_clause,
    );
    connection
        .query_row(&sql, params_from_iter(filter_sql.params.iter()), |row| {
            Ok(CommandReportStatusSummary {
                total: row.get(0)?,
                open: row.get(1)?,
                resolved: row.get(2)?,
                native_parity: row.get(3)?,
                not_reproducible: row.get(4)?,
                denied: row.get(5)?,
                other: row.get(6)?,
            })
        })
        .map_err(Into::into)
}

pub fn command_report_denial_reason_summary(
    filter: CommandFilter<'_>,
) -> Result<CommandReportDenialReasonSummary> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(CommandReportDenialReasonSummary::default());
    };
    if !table_exists(&connection, "command_reports")? {
        return Ok(CommandReportDenialReasonSummary::default());
    }
    let expressions = ReportDispositionExpressions::load(&connection)?;
    let filter_sql = command_report_filter_sql(
        &connection,
        filter,
        Some(CommandReportStatus::Denied),
        &expressions.status,
    )?;
    let known_reasons =
        "'duplicate', 'insufficient_evidence', 'invalid', 'obsolete', 'unsupported', 'low_value'";
    let sql = format!(
        "
        SELECT COUNT(*) AS total,
               COALESCE(SUM(CASE WHEN {} = 'duplicate' THEN 1 ELSE 0 END), 0) AS duplicate,
               COALESCE(SUM(CASE WHEN {} = 'insufficient_evidence' THEN 1 ELSE 0 END), 0) AS insufficient_evidence,
               COALESCE(SUM(CASE WHEN {} = 'invalid' THEN 1 ELSE 0 END), 0) AS invalid,
               COALESCE(SUM(CASE WHEN {} = 'obsolete' THEN 1 ELSE 0 END), 0) AS obsolete,
               COALESCE(SUM(CASE WHEN {} = 'unsupported' THEN 1 ELSE 0 END), 0) AS unsupported,
               COALESCE(SUM(CASE WHEN {} = 'low_value' THEN 1 ELSE 0 END), 0) AS low_value,
               COALESCE(SUM(CASE WHEN {} NOT IN ({known_reasons}) THEN 1 ELSE 0 END), 0) AS other
        FROM command_reports
        {}
        ",
        expressions.denial_reason,
        expressions.denial_reason,
        expressions.denial_reason,
        expressions.denial_reason,
        expressions.denial_reason,
        expressions.denial_reason,
        expressions.denial_reason,
        filter_sql.where_clause,
    );
    connection
        .query_row(&sql, params_from_iter(filter_sql.params.iter()), |row| {
            Ok(CommandReportDenialReasonSummary {
                total: row.get(0)?,
                duplicate: row.get(1)?,
                insufficient_evidence: row.get(2)?,
                invalid: row.get(3)?,
                obsolete: row.get(4)?,
                unsupported: row.get(5)?,
                low_value: row.get(6)?,
                other: row.get(7)?,
            })
        })
        .map_err(Into::into)
}

struct ReportDispositionExpressions {
    status: String,
    note: String,
    revision: String,
    denial_reason: String,
    related_report_id: String,
    updated_at_ms: String,
}

struct ReportEvidenceExpressions {
    invocation_id: String,
    cwd: String,
    git_root: String,
    thread_id: String,
    command_shape: String,
    command_shape_hash: String,
    evidence_kind: String,
    cx_response: String,
    native_response: String,
    artifact_ref: String,
    binary_version: String,
    binary_revision: String,
    binary_fingerprint: String,
}

impl ReportEvidenceExpressions {
    fn load(connection: &Connection) -> Result<Self> {
        if !table_exists(connection, "command_report_evidence")? {
            return Ok(Self::empty());
        }
        Ok(Self {
            invocation_id: evidence_value("invocation_id", "NULL"),
            cwd: evidence_value("cwd", "''"),
            git_root: evidence_value("git_root", "''"),
            thread_id: evidence_value("thread_id", "''"),
            command_shape: evidence_value("command_shape", "''"),
            command_shape_hash: evidence_value("command_shape_hash", "''"),
            evidence_kind: if column_exists(connection, "command_report_evidence", "evidence_kind")?
            {
                evidence_value("evidence_kind", "'legacy'")
            } else {
                "'legacy'".to_string()
            },
            cx_response: evidence_value("cx_response", "''"),
            native_response: evidence_value("native_response", "''"),
            artifact_ref: evidence_value("artifact_ref", "''"),
            binary_version: evidence_value("binary_version", "''"),
            binary_revision: evidence_value("binary_revision", "''"),
            binary_fingerprint: evidence_value("binary_fingerprint", "''"),
        })
    }

    fn empty() -> Self {
        Self {
            invocation_id: "NULL".to_string(),
            cwd: "''".to_string(),
            git_root: "''".to_string(),
            thread_id: "''".to_string(),
            command_shape: "''".to_string(),
            command_shape_hash: "''".to_string(),
            evidence_kind: "'legacy'".to_string(),
            cx_response: "''".to_string(),
            native_response: "''".to_string(),
            artifact_ref: "''".to_string(),
            binary_version: "''".to_string(),
            binary_revision: "''".to_string(),
            binary_fingerprint: "''".to_string(),
        }
    }
}

impl ReportDispositionExpressions {
    fn load(connection: &Connection) -> Result<Self> {
        if !table_exists(connection, DISPOSITION_TABLE)? {
            return Ok(Self {
                status: "'open'".to_string(),
                note: "''".to_string(),
                revision: "''".to_string(),
                denial_reason: "''".to_string(),
                related_report_id: "NULL".to_string(),
                updated_at_ms: "0".to_string(),
            });
        }
        Ok(Self {
            status: disposition_value("status", "'open'"),
            note: disposition_value("note", "''"),
            revision: disposition_value("revision", "''"),
            denial_reason: if column_exists(connection, DISPOSITION_TABLE, "denial_reason")? {
                disposition_value("denial_reason", "''")
            } else {
                "''".to_string()
            },
            related_report_id: if column_exists(connection, DISPOSITION_TABLE, "related_report_id")?
            {
                disposition_value("related_report_id", "NULL")
            } else {
                "NULL".to_string()
            },
            updated_at_ms: disposition_value("updated_at_ms", "0"),
        })
    }
}

fn disposition_value(column: &str, fallback: &str) -> String {
    format!(
        "COALESCE((SELECT {column} FROM {DISPOSITION_TABLE} WHERE report_id = command_reports.id), {fallback})"
    )
}

fn evidence_value(column: &str, fallback: &str) -> String {
    format!(
        "COALESCE((SELECT {column} FROM command_report_evidence WHERE report_id = command_reports.id), {fallback})"
    )
}

struct FilterSql {
    where_clause: String,
    params: Vec<String>,
}

fn command_report_filter_sql(
    connection: &Connection,
    filter: CommandFilter<'_>,
    status: Option<CommandReportStatus>,
    status_expression: &str,
) -> Result<FilterSql> {
    let mut conditions = Vec::new();
    let mut params = Vec::new();
    if let Some(root) = filter.command_root.filter(|value| !value.is_empty()) {
        let root_expression = command_report_root_query_expression(connection)?;
        conditions.push(format!("({root_expression}) = ?"));
        params.push(root.to_string());
    }
    if let Some(command) = filter.command.filter(|value| !value.is_empty()) {
        conditions.push("command_family = ?".to_string());
        params.push(command.to_string());
    }
    if let Some(status) = status {
        conditions.push(format!("({status_expression}) = ?"));
        params.push(status.as_str().to_string());
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    Ok(FilterSql {
        where_clause,
        params,
    })
}
