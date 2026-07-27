use std::collections::BTreeMap;
use std::fs;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, TransactionBehavior};

use super::queries::command_report_root_query_expression;
use super::store::{column_exists, create_schema, open_readonly_database, table_exists};
use super::{
    insights_db_file, now_ms, CommandReportDenialReason, CommandReportDenialReasonSummary,
    CommandReportStatusSummary, CommandReportTriageProposal, CommandReportTriageReceipt,
    GENERIC_COMMAND_REPORT_NOTE,
};

const TRIAGE_REVISION: &str = "r132";
const DUPLICATE_RULE_ID: &str = "exact-duplicate-v1";
const INVALID_RULE_ID: &str = "bare-command-without-actionable-evidence-v2";
const INSUFFICIENT_EVIDENCE_RULE_ID: &str = "generic-note-without-actionable-evidence-v2";

pub(super) fn command_report_triage(apply: bool) -> Result<CommandReportTriageReceipt> {
    if apply {
        apply_triage()
    } else {
        preview_triage()
    }
}

fn preview_triage() -> Result<CommandReportTriageReceipt> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(empty_receipt(false));
    };
    if !table_exists(&connection, "command_reports")? {
        return Ok(empty_receipt(false));
    }
    build_receipt(&connection, false, 0, 0)
}

fn apply_triage() -> Result<CommandReportTriageReceipt> {
    let db_path = insights_db_file()?;
    if !db_path.is_file() {
        bail!(
            "CX insights database does not exist at {}",
            db_path.display()
        );
    }
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut connection = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection.busy_timeout(super::COMMAND_REPORT_BUSY_TIMEOUT)?;
    create_schema(&connection)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = status_summary(&transaction)?;
    let proposals = classify_open_reports(&transaction)?;
    let denial_reasons = denial_reason_summary(&proposals);
    let projected = projected_status(current, proposals.len() as u64);
    let updated_at_ms = now_ms();
    let mut applied_count = 0u64;
    for proposal in &proposals {
        let changed = transaction.execute(
            "
            INSERT INTO command_report_dispositions (
                report_id, status, note, revision, denial_reason,
                related_report_id, updated_at_ms
            )
            VALUES (?1, 'denied', ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(report_id) DO UPDATE SET
                status = excluded.status,
                note = excluded.note,
                revision = excluded.revision,
                denial_reason = excluded.denial_reason,
                related_report_id = excluded.related_report_id,
                updated_at_ms = excluded.updated_at_ms
            WHERE command_report_dispositions.status = 'open'
            ",
            params![
                proposal.report_id,
                proposal.note,
                TRIAGE_REVISION,
                proposal.denial_reason.as_str(),
                proposal.related_report_id,
                updated_at_ms,
            ],
        )?;
        applied_count += changed as u64;
    }
    let final_status = status_summary(&transaction)?;
    transaction.commit()?;
    Ok(CommandReportTriageReceipt {
        applied: true,
        current,
        projected,
        final_status,
        denial_reasons,
        skipped_count: proposals.len() as u64 - applied_count,
        proposals,
        applied_count,
    })
}

fn build_receipt(
    connection: &Connection,
    applied: bool,
    applied_count: u64,
    skipped_count: u64,
) -> Result<CommandReportTriageReceipt> {
    let current = status_summary(connection)?;
    let proposals = classify_open_reports(connection)?;
    let denial_reasons = denial_reason_summary(&proposals);
    let projected = projected_status(current, proposals.len() as u64);
    Ok(CommandReportTriageReceipt {
        applied,
        current,
        projected,
        final_status: current,
        denial_reasons,
        proposals,
        applied_count,
        skipped_count,
    })
}

fn empty_receipt(applied: bool) -> CommandReportTriageReceipt {
    CommandReportTriageReceipt {
        applied,
        current: CommandReportStatusSummary::default(),
        projected: CommandReportStatusSummary::default(),
        final_status: CommandReportStatusSummary::default(),
        denial_reasons: CommandReportDenialReasonSummary::default(),
        proposals: Vec::new(),
        applied_count: 0,
        skipped_count: 0,
    }
}

fn projected_status(
    current: CommandReportStatusSummary,
    proposal_count: u64,
) -> CommandReportStatusSummary {
    CommandReportStatusSummary {
        open: current.open.saturating_sub(proposal_count),
        denied: current.denied + proposal_count,
        ..current
    }
}

#[derive(Debug, Clone)]
struct TriageCandidate {
    id: u64,
    occurred_at_ms: u64,
    command_root: String,
    command: String,
    command_family: String,
    issue_kind: String,
    note: String,
    evidence_kind: String,
    cx_response_bytes: u64,
    native_response_bytes: u64,
    invocation_id: u64,
    artifact_ref_bytes: u64,
}

impl TriageCandidate {
    fn has_response(&self) -> bool {
        self.cx_response_bytes > 0 || self.native_response_bytes > 0
    }

    fn has_actionable_evidence(&self) -> bool {
        self.has_response() || self.invocation_id > 0 || self.artifact_ref_bytes > 0
    }

    fn evidence_score(&self) -> u8 {
        match (self.cx_response_bytes > 0, self.native_response_bytes > 0) {
            (true, true) => 7,
            (true, false) => 6,
            (false, true) => 5,
            (false, false) if self.artifact_ref_bytes > 0 => 4,
            (false, false) if self.invocation_id > 0 => 3,
            (false, false) if self.evidence_kind.ends_with(":metadata-only") => 2,
            (false, false) => 0,
        }
    }
}

fn classify_open_reports(connection: &Connection) -> Result<Vec<CommandReportTriageProposal>> {
    let candidates = load_open_candidates(connection)?;
    let mut duplicate_groups = BTreeMap::<(String, String, String, String), Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        duplicate_groups
            .entry((
                candidate.command_root.clone(),
                candidate.command_family.clone(),
                candidate.command.clone(),
                candidate.issue_kind.clone(),
            ))
            .or_default()
            .push(index);
    }

    let mut proposals = BTreeMap::<u64, CommandReportTriageProposal>::new();
    for indexes in duplicate_groups
        .values()
        .filter(|indexes| indexes.len() > 1)
    {
        let canonical_index = indexes
            .iter()
            .copied()
            .max_by_key(|index| {
                let candidate = &candidates[*index];
                (
                    candidate.evidence_score(),
                    candidate.occurred_at_ms,
                    candidate.id,
                )
            })
            .expect("duplicate groups are non-empty");
        let canonical_id = candidates[canonical_index].id;
        for index in indexes {
            let candidate = &candidates[*index];
            if *index == canonical_index || candidate.has_response() {
                continue;
            }
            proposals.insert(
                candidate.id,
                proposal(
                    candidate,
                    CommandReportDenialReason::Duplicate,
                    Some(canonical_id),
                    DUPLICATE_RULE_ID,
                    format!(
                        "Denied by {TRIAGE_REVISION} conservative triage: exact duplicate of report {canonical_id}; canonical row selected by response-evidence quality and recency."
                    ),
                ),
            );
        }
    }

    for candidate in &candidates {
        if proposals.contains_key(&candidate.id) || candidate.has_actionable_evidence() {
            continue;
        }
        if is_bare_command(&candidate.command) {
            proposals.insert(
                candidate.id,
                proposal(
                    candidate,
                    CommandReportDenialReason::Invalid,
                    None,
                    INVALID_RULE_ID,
                    format!(
                        "Denied by {TRIAGE_REVISION} conservative triage: the report identifies only a bare command runner and contains no response, invocation, or artifact evidence."
                    ),
                ),
            );
        } else if candidate.note == GENERIC_COMMAND_REPORT_NOTE {
            proposals.insert(
                candidate.id,
                proposal(
                    candidate,
                    CommandReportDenialReason::InsufficientEvidence,
                    None,
                    INSUFFICIENT_EVIDENCE_RULE_ID,
                    format!(
                        "Denied by {TRIAGE_REVISION} conservative triage: no response, invocation, or artifact evidence was recorded and the report contains only the generic placeholder note. Create a new report or reopen this row if the issue is reproduced with current evidence."
                    ),
                ),
            );
        }
    }
    Ok(proposals.into_values().collect())
}

fn proposal(
    candidate: &TriageCandidate,
    denial_reason: CommandReportDenialReason,
    related_report_id: Option<u64>,
    policy_rule_id: &str,
    note: String,
) -> CommandReportTriageProposal {
    CommandReportTriageProposal {
        report_id: candidate.id,
        denial_reason,
        related_report_id,
        policy_rule_id: policy_rule_id.to_string(),
        command_root: candidate.command_root.clone(),
        command_family: candidate.command_family.clone(),
        command: candidate.command.clone(),
        evidence_kind: candidate.evidence_kind.clone(),
        cx_response_recorded: candidate.cx_response_bytes > 0,
        native_response_recorded: candidate.native_response_bytes > 0,
        invocation_recorded: candidate.invocation_id > 0,
        artifact_recorded: candidate.artifact_ref_bytes > 0,
        actionable_evidence_recorded: candidate.has_actionable_evidence(),
        note,
    }
}

fn is_bare_command(command: &str) -> bool {
    matches!(command.trim(), "" | "sh" | "bash" | "--" | "cx")
}

fn denial_reason_summary(
    proposals: &[CommandReportTriageProposal],
) -> CommandReportDenialReasonSummary {
    let mut summary = CommandReportDenialReasonSummary::default();
    for proposal in proposals {
        summary.record(proposal.denial_reason);
    }
    summary
}

fn load_open_candidates(connection: &Connection) -> Result<Vec<TriageCandidate>> {
    let command_root = command_report_root_query_expression(connection)?;
    let status = disposition_expression(connection, "status", "'open'")?;
    let evidence_kind = evidence_column_expression(connection, "evidence_kind", "'legacy'", false)?;
    let cx_response_bytes = evidence_column_expression(connection, "cx_response", "0", true)?;
    let native_response_bytes =
        evidence_column_expression(connection, "native_response", "0", true)?;
    let invocation_id = evidence_column_expression(connection, "invocation_id", "0", false)?;
    let artifact_ref_bytes = evidence_column_expression(connection, "artifact_ref", "0", true)?;
    let sql = format!(
        "
        SELECT id, occurred_at_ms, {command_root} AS command_root, command,
               command_family, issue_kind, note, {evidence_kind} AS evidence_kind,
               {cx_response_bytes} AS cx_response_bytes,
               {native_response_bytes} AS native_response_bytes,
               {invocation_id} AS invocation_id,
               {artifact_ref_bytes} AS artifact_ref_bytes
        FROM command_reports
        WHERE ({status}) = 'open'
        ORDER BY occurred_at_ms ASC, id ASC
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        Ok(TriageCandidate {
            id: row.get(0)?,
            occurred_at_ms: row.get(1)?,
            command_root: row.get(2)?,
            command: row.get(3)?,
            command_family: row.get(4)?,
            issue_kind: row.get(5)?,
            note: row.get(6)?,
            evidence_kind: row.get(7)?,
            cx_response_bytes: row.get(8)?,
            native_response_bytes: row.get(9)?,
            invocation_id: row.get(10)?,
            artifact_ref_bytes: row.get(11)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn status_summary(connection: &Connection) -> Result<CommandReportStatusSummary> {
    if !table_exists(connection, "command_reports")? {
        return Ok(CommandReportStatusSummary::default());
    }
    let status = disposition_expression(connection, "status", "'open'")?;
    let known_statuses = "'open', 'resolved', 'native_parity', 'not_reproducible', 'denied'";
    let sql = format!(
        "
        SELECT COUNT(*) AS total,
               COALESCE(SUM(CASE WHEN {status} = 'open' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN {status} = 'resolved' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN {status} = 'native_parity' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN {status} = 'not_reproducible' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN {status} = 'denied' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN {status} NOT IN ({known_statuses}) THEN 1 ELSE 0 END), 0)
        FROM command_reports
        "
    );
    connection
        .query_row(&sql, [], |row| {
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

fn disposition_expression(connection: &Connection, column: &str, fallback: &str) -> Result<String> {
    if !table_exists(connection, "command_report_dispositions")? {
        return Ok(fallback.to_string());
    }
    Ok(format!(
        "COALESCE((SELECT {column} FROM command_report_dispositions WHERE report_id = command_reports.id), {fallback})"
    ))
}

fn evidence_column_expression(
    connection: &Connection,
    column: &str,
    fallback: &str,
    length: bool,
) -> Result<String> {
    if !table_exists(connection, "command_report_evidence")?
        || !column_exists(connection, "command_report_evidence", column)?
    {
        return Ok(fallback.to_string());
    }
    let value = if length {
        format!("LENGTH({column})")
    } else {
        column.to_string()
    };
    Ok(format!(
        "COALESCE((SELECT {value} FROM command_report_evidence WHERE report_id = command_reports.id), {fallback})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    struct CandidateEvidence {
        kind: &'static str,
        cx_response_bytes: u64,
        native_response_bytes: u64,
        invocation_id: u64,
        artifact_ref_bytes: u64,
    }

    impl CandidateEvidence {
        const fn none(kind: &'static str) -> Self {
            Self {
                kind,
                cx_response_bytes: 0,
                native_response_bytes: 0,
                invocation_id: 0,
                artifact_ref_bytes: 0,
            }
        }

        const fn cx_response(kind: &'static str, bytes: u64) -> Self {
            Self {
                cx_response_bytes: bytes,
                ..Self::none(kind)
            }
        }

        const fn invocation(kind: &'static str, invocation_id: u64) -> Self {
            Self {
                invocation_id,
                ..Self::none(kind)
            }
        }

        const fn artifact(kind: &'static str, bytes: u64) -> Self {
            Self {
                artifact_ref_bytes: bytes,
                ..Self::none(kind)
            }
        }
    }

    fn candidate(
        id: u64,
        occurred_at_ms: u64,
        command: &str,
        note: &str,
        evidence: CandidateEvidence,
    ) -> TriageCandidate {
        TriageCandidate {
            id,
            occurred_at_ms,
            command_root: command.split_whitespace().next().unwrap_or("").to_string(),
            command: command.to_string(),
            command_family: command.to_string(),
            issue_kind: "suspicious_output".to_string(),
            note: note.to_string(),
            evidence_kind: evidence.kind.to_string(),
            cx_response_bytes: evidence.cx_response_bytes,
            native_response_bytes: evidence.native_response_bytes,
            invocation_id: evidence.invocation_id,
            artifact_ref_bytes: evidence.artifact_ref_bytes,
        }
    }

    fn classifier_connection() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE command_reports (
                    id INTEGER PRIMARY KEY,
                    occurred_at_ms INTEGER NOT NULL,
                    command TEXT NOT NULL,
                    command_root TEXT NOT NULL,
                    command_family TEXT NOT NULL,
                    issue_kind TEXT NOT NULL,
                    note TEXT NOT NULL
                );
                CREATE TABLE command_report_evidence (
                    report_id INTEGER PRIMARY KEY,
                    evidence_kind TEXT NOT NULL,
                    cx_response TEXT NOT NULL,
                    native_response TEXT NOT NULL,
                    invocation_id INTEGER,
                    artifact_ref TEXT NOT NULL
                );
                ",
            )
            .unwrap();
        connection
    }

    fn insert_candidate(connection: &Connection, row: TriageCandidate) {
        connection
            .execute(
                "INSERT INTO command_reports VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.id,
                    row.occurred_at_ms,
                    row.command,
                    row.command_root,
                    row.command_family,
                    row.issue_kind,
                    row.note,
                ],
            )
            .unwrap();
        if row.has_actionable_evidence() {
            connection
                .execute(
                    "INSERT INTO command_report_evidence VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        row.id,
                        row.evidence_kind,
                        "x".repeat(row.cx_response_bytes as usize),
                        "x".repeat(row.native_response_bytes as usize),
                        (row.invocation_id > 0).then_some(row.invocation_id),
                        "x".repeat(row.artifact_ref_bytes as usize),
                    ],
                )
                .unwrap();
        }
    }

    #[test]
    fn classifier_protects_response_and_specific_note_evidence() {
        let connection = classifier_connection();
        let rows = [
            candidate(
                1,
                10,
                "git diff",
                GENERIC_COMMAND_REPORT_NOTE,
                CandidateEvidence::none("legacy"),
            ),
            candidate(
                2,
                20,
                "git diff",
                GENERIC_COMMAND_REPORT_NOTE,
                CandidateEvidence::cx_response("exact-command:failure-detail", 10),
            ),
            candidate(
                3,
                30,
                "bash",
                GENERIC_COMMAND_REPORT_NOTE,
                CandidateEvidence::none("legacy"),
            ),
            candidate(
                4,
                40,
                "grep route",
                "specific reproduction",
                CandidateEvidence::none("legacy"),
            ),
            candidate(
                5,
                50,
                "ls",
                GENERIC_COMMAND_REPORT_NOTE,
                CandidateEvidence::none("legacy"),
            ),
            candidate(
                6,
                60,
                "node script.mjs --check",
                GENERIC_COMMAND_REPORT_NOTE,
                CandidateEvidence::invocation("exact-command:metadata-only", 42),
            ),
            candidate(
                7,
                70,
                "cmake --build build",
                GENERIC_COMMAND_REPORT_NOTE,
                CandidateEvidence::artifact("exact-command:failure-detail", 24),
            ),
        ];
        for row in rows {
            insert_candidate(&connection, row);
        }

        let proposals = classify_open_reports(&connection).unwrap();
        assert_eq!(
            proposals
                .iter()
                .map(|proposal| (proposal.report_id, proposal.denial_reason))
                .collect::<Vec<_>>(),
            vec![
                (1, CommandReportDenialReason::Duplicate),
                (3, CommandReportDenialReason::Invalid),
                (5, CommandReportDenialReason::InsufficientEvidence),
            ]
        );
        assert_eq!(proposals[0].related_report_id, Some(2));
    }
}
