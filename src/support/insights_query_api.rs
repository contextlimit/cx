use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{params_from_iter, Connection};

use super::queries::{
    argv_json_query_expression, classified_failure_sql, command_family_query_expression,
    legacy_process_sql, nullable_integer_column_query_expression_for_table,
    optional_integer_column_query_expression, optional_text_column_query_expression,
    optional_text_column_query_expression_for_table, process_query_expression,
    source_query_expression,
};
use super::{
    insights_db_file, open_readonly_database, table_exists, CommandFilter, CommandLevel,
    CommandTotalInsight, CommandTotalSort, DailyInsight, FailureCoverageInsight,
    FailureDetailInsight, InvocationInsight, OverallInsight, SavingsDistributionInsight,
    SavingsSort, TextMetrics,
};

const SAVED_TOKENS_INDEX: &str = "idx_command_invocations_saved_tokens";

pub fn command_totals(sort: CommandTotalSort, limit: usize) -> Result<Vec<CommandTotalInsight>> {
    command_totals_at_level(CommandLevel::Command, sort, limit)
}

pub fn command_totals_at_level(
    level: CommandLevel,
    sort: CommandTotalSort,
    limit: usize,
) -> Result<Vec<CommandTotalInsight>> {
    command_totals_at_level_filtered(level, sort, limit, CommandFilter::default())
}

pub fn command_totals_at_level_filtered(
    level: CommandLevel,
    sort: CommandTotalSort,
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<CommandTotalInsight>> {
    if !filter.is_empty() {
        return command_invocation_totals_at_level(level, sort, limit, filter);
    }
    match level {
        CommandLevel::Command => command_leaf_totals(sort, limit),
        CommandLevel::Root => command_root_totals(sort, limit),
    }
}

fn command_leaf_totals(sort: CommandTotalSort, limit: usize) -> Result<Vec<CommandTotalInsight>> {
    command_invocation_totals_at_level(CommandLevel::Command, sort, limit, CommandFilter::default())
}

fn command_root_totals(sort: CommandTotalSort, limit: usize) -> Result<Vec<CommandTotalInsight>> {
    command_invocation_totals_at_level(CommandLevel::Root, sort, limit, CommandFilter::default())
}

fn command_invocation_totals_at_level(
    level: CommandLevel,
    sort: CommandTotalSort,
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<CommandTotalInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_invocations")? {
        return Ok(Vec::new());
    }
    let process = process_query_expression(&connection, None)?;
    let command_family = command_family_query_expression(&connection, None)?;
    let classified_failure = classified_failure_sql(&connection, None)?;
    let expansion = ExpansionSql::load(&connection, None)?;
    let group_expression = match level {
        CommandLevel::Command => command_family,
        CommandLevel::Root => process,
    };
    let filter_sql = invocation_filter_sql(&connection, filter)?;

    let sql = format!(
        "
        SELECT {group_expression} AS command,
               COUNT(*) AS invocations, SUM({classified_failure}) AS failures,
               SUM(raw_bytes) AS raw_bytes, SUM(raw_chars) AS raw_chars,
               SUM(raw_lines) AS raw_lines, SUM(raw_tokens) AS raw_tokens,
               SUM(emitted_bytes) AS emitted_bytes, SUM(emitted_chars) AS emitted_chars,
               SUM(emitted_lines) AS emitted_lines, SUM(emitted_tokens) AS emitted_tokens,
               SUM(saved_bytes) AS saved_bytes, SUM(saved_chars) AS saved_chars,
               SUM(saved_lines) AS saved_lines, SUM(saved_tokens) AS saved_tokens,
               SUM(CASE WHEN {} THEN 1 ELSE 0 END) AS expansions,
               SUM({}) AS expanded_bytes, SUM({}) AS expanded_chars,
               SUM({}) AS expanded_lines, SUM({}) AS expanded_tokens,
               MAX(saved_chars) AS best_saved_chars, MAX(saved_tokens) AS best_saved_tokens,
               MAX({}) AS best_expanded_tokens
        FROM command_invocations
        {}
        GROUP BY 1
        ORDER BY {} DESC, command ASC
        LIMIT ?
        ",
        expansion.predicate(),
        expansion.bytes,
        expansion.chars,
        expansion.lines,
        expansion.tokens,
        expansion.tokens,
        filter_sql.where_clause,
        command_total_order_column(sort)
    );
    let mut statement = connection.prepare(&sql)?;
    let mut params = filter_sql.params;
    params.push(bounded_limit(limit).to_string());
    let rows = statement.query_map(params_from_iter(params.iter()), command_total_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn command_total_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CommandTotalInsight> {
    Ok(CommandTotalInsight {
        command: row.get(0)?,
        invocations: row.get(1)?,
        failures: row.get(2)?,
        raw: TextMetrics {
            bytes: row.get(3)?,
            chars: row.get(4)?,
            lines: row.get(5)?,
            tokens: row.get(6)?,
        },
        emitted: TextMetrics {
            bytes: row.get(7)?,
            chars: row.get(8)?,
            lines: row.get(9)?,
            tokens: row.get(10)?,
        },
        saved: TextMetrics {
            bytes: row.get(11)?,
            chars: row.get(12)?,
            lines: row.get(13)?,
            tokens: row.get(14)?,
        },
        expansions: row.get(15)?,
        expanded: TextMetrics {
            bytes: row.get(16)?,
            chars: row.get(17)?,
            lines: row.get(18)?,
            tokens: row.get(19)?,
        },
        best_saved_chars: row.get(20)?,
        best_saved_tokens: row.get(21)?,
        best_expanded_tokens: row.get(22)?,
    })
}

pub fn failure_coverage_at_level_filtered(
    level: CommandLevel,
    filter: CommandFilter<'_>,
) -> Result<Vec<FailureCoverageInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    let mut coverage = BTreeMap::<String, FailureCoverageInsight>::new();
    if table_exists(&connection, "command_invocations")? {
        load_failed_invocation_coverage(&connection, level, filter, &mut coverage)?;
    }
    if table_exists(&connection, "command_failures")? {
        load_failure_detail_coverage(&connection, level, filter, &mut coverage)?;
    }
    for row in coverage.values_mut() {
        row.unknown_invocations = row
            .failed_invocations
            .saturating_sub(row.linked_invocations);
    }
    let mut rows = coverage.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .output_gap_detail_rows
            .cmp(&left.output_gap_detail_rows)
            .then_with(|| right.unknown_invocations.cmp(&left.unknown_invocations))
            .then_with(|| right.failed_invocations.cmp(&left.failed_invocations))
            .then_with(|| left.command.cmp(&right.command))
    });
    Ok(rows)
}

fn load_failed_invocation_coverage(
    connection: &Connection,
    level: CommandLevel,
    filter: CommandFilter<'_>,
    coverage: &mut BTreeMap<String, FailureCoverageInsight>,
) -> Result<()> {
    let process = process_query_expression(connection, Some("ci"))?;
    let command_family = command_family_query_expression(connection, Some("ci"))?;
    let classified_failure = classified_failure_sql(connection, Some("ci"))?;
    let group_expression = match level {
        CommandLevel::Command => command_family,
        CommandLevel::Root => process,
    };
    let filter_sql = invocation_filter_sql_with_alias(connection, filter, Some("ci"))?;
    let where_clause = filter_sql.with_condition(&format!("({classified_failure}) = 1"));
    let sql = format!(
        "
        SELECT {group_expression} AS command, COUNT(*) AS failed_invocations
        FROM command_invocations ci
        {where_clause}
        GROUP BY 1
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter_sql.params.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
    })?;
    for row in rows {
        let (command, failed_invocations) = row?;
        let item = coverage
            .entry(command.clone())
            .or_insert_with(|| FailureCoverageInsight {
                command,
                ..FailureCoverageInsight::default()
            });
        item.failed_invocations = failed_invocations;
    }
    Ok(())
}

fn load_failure_detail_coverage(
    connection: &Connection,
    level: CommandLevel,
    filter: CommandFilter<'_>,
    coverage: &mut BTreeMap<String, FailureCoverageInsight>,
) -> Result<()> {
    let query = FailureDetailCoverageQuery::load(connection, level)?;
    let filter_sql =
        failure_detail_filter_sql(filter, &query.effective_root, &query.effective_family);
    let sql = query.sql(&filter_sql.where_clause);
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(filter_sql.params.iter()),
        failure_detail_coverage_from_row,
    )?;
    for row in rows {
        merge_failure_detail_coverage(
            coverage,
            row?,
            query.response_evidence_available,
            query.artifact_reference_available,
        );
    }
    for item in coverage.values_mut() {
        item.response_evidence_available = query.response_evidence_available;
        item.artifact_reference_available = query.artifact_reference_available;
    }
    Ok(())
}

struct FailureDetailCoverageQuery {
    group_expression: String,
    effective_root: String,
    effective_family: String,
    linked: String,
    invocation_id: String,
    join_clause: String,
    output_bearing: String,
    artifact_linked: String,
    response_evidence_available: bool,
    artifact_reference_available: bool,
}

impl FailureDetailCoverageQuery {
    fn load(connection: &Connection, level: CommandLevel) -> Result<Self> {
        let stored_family = optional_text_column_query_expression_for_table(
            connection,
            "command_failures",
            Some("cf"),
            "command_family",
        )?;
        let stored_root = failure_detail_root_expression(connection, &stored_family)?;
        let invocation_id_available =
            super::column_exists(connection, "command_failures", "invocation_id")?;
        let can_join = invocation_id_available && table_exists(connection, "command_invocations")?;
        let (linked, effective_root, effective_family, join_clause) =
            effective_failure_detail_identity(connection, can_join, &stored_root, &stored_family)?;
        let invocation_id = nullable_integer_column_query_expression_for_table(
            connection,
            "command_failures",
            Some("cf"),
            "invocation_id",
        )?;
        let cx_response = optional_text_column_query_expression_for_table(
            connection,
            "command_failures",
            Some("cf"),
            "cx_response",
        )?;
        let raw_response = optional_text_column_query_expression_for_table(
            connection,
            "command_failures",
            Some("cf"),
            "raw_response",
        )?;
        let artifact_ref = optional_text_column_query_expression_for_table(
            connection,
            "command_failures",
            Some("cf"),
            "artifact_ref",
        )?;
        let response_evidence_available =
            super::column_exists(connection, "command_failures", "cx_response")?
                || super::column_exists(connection, "command_failures", "raw_response")?;
        let artifact_reference_available =
            super::column_exists(connection, "command_failures", "artifact_ref")?;
        let group_expression = match level {
            CommandLevel::Command => effective_family.clone(),
            CommandLevel::Root => effective_root.clone(),
        };
        Ok(Self {
            group_expression,
            effective_root,
            effective_family,
            linked,
            invocation_id,
            join_clause,
            output_bearing: format!(
                "(LENGTH(COALESCE({cx_response}, '')) > 0 OR LENGTH(COALESCE({raw_response}, '')) > 0)"
            ),
            artifact_linked: format!("LENGTH(TRIM(COALESCE({artifact_ref}, ''))) > 0"),
            response_evidence_available,
            artifact_reference_available,
        })
    }

    fn sql(&self, where_clause: &str) -> String {
        let output_count = aggregate_count(&self.output_bearing, self.response_evidence_available);
        let silent_count = aggregate_count(
            &format!("NOT {}", self.output_bearing),
            self.response_evidence_available,
        );
        let artifact_count =
            aggregate_count(&self.artifact_linked, self.artifact_reference_available);
        let output_gap_count = aggregate_count(
            &format!("{} AND NOT ({})", self.output_bearing, self.artifact_linked),
            self.response_evidence_available && self.artifact_reference_available,
        );
        let group = &self.group_expression;
        let linked = &self.linked;
        let invocation_id = &self.invocation_id;
        let join = &self.join_clause;
        format!(
            "
            SELECT {group} AS command, COUNT(*) AS detail_rows,
                   SUM(CASE WHEN {linked} THEN 1 ELSE 0 END),
                   COUNT(DISTINCT CASE WHEN {linked} THEN {invocation_id} END),
                   SUM(CASE WHEN NOT ({linked}) THEN 1 ELSE 0 END),
                   {output_count}, {silent_count}, {artifact_count}, {output_gap_count}
            FROM command_failures cf
            {join}
            {where_clause}
            GROUP BY 1
            "
        )
    }
}

type FailureDetailCoverageRow = (String, u64, u64, u64, u64, u64, u64, u64, u64);

fn failure_detail_coverage_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FailureDetailCoverageRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn merge_failure_detail_coverage(
    coverage: &mut BTreeMap<String, FailureCoverageInsight>,
    row: FailureDetailCoverageRow,
    response_evidence_available: bool,
    artifact_reference_available: bool,
) {
    let item = coverage
        .entry(row.0.clone())
        .or_insert_with(|| FailureCoverageInsight {
            command: row.0,
            ..FailureCoverageInsight::default()
        });
    item.detail_rows = row.1;
    item.linked_detail_rows = row.2;
    item.linked_invocations = row.3;
    item.orphan_detail_rows = row.4;
    item.output_bearing_detail_rows = row.5;
    item.silent_detail_rows = row.6;
    item.artifact_linked_detail_rows = row.7;
    item.output_gap_detail_rows = row.8;
    item.response_evidence_available = response_evidence_available;
    item.artifact_reference_available = artifact_reference_available;
}

fn failure_detail_root_expression(connection: &Connection, family: &str) -> Result<String> {
    if super::column_exists(connection, "command_failures", "command_root")? {
        Ok(format!(
            "COALESCE(NULLIF(cf.command_root, ''), {})",
            legacy_process_sql(family)
        ))
    } else {
        Ok(legacy_process_sql(family))
    }
}

fn effective_failure_detail_identity(
    connection: &Connection,
    can_join: bool,
    stored_root: &str,
    stored_family: &str,
) -> Result<(String, String, String, String)> {
    if !can_join {
        return Ok((
            "0".to_string(),
            stored_root.to_string(),
            stored_family.to_string(),
            String::new(),
        ));
    }
    let invocation_root = process_query_expression(connection, Some("ci"))?;
    let invocation_family = command_family_query_expression(connection, Some("ci"))?;
    let classified_failure = classified_failure_sql(connection, Some("ci"))?;
    let linked = format!("ci.id IS NOT NULL AND ({classified_failure}) = 1");
    Ok((
        linked.clone(),
        format!("CASE WHEN {linked} THEN {invocation_root} ELSE {stored_root} END"),
        format!("CASE WHEN {linked} THEN {invocation_family} ELSE {stored_family} END"),
        "LEFT JOIN command_invocations ci ON ci.id = cf.invocation_id".to_string(),
    ))
}

fn aggregate_count(predicate: &str, available: bool) -> String {
    if available {
        format!("SUM(CASE WHEN {predicate} THEN 1 ELSE 0 END)")
    } else {
        "0".to_string()
    }
}

fn failure_detail_filter_sql(
    filter: CommandFilter<'_>,
    command_root: &str,
    command_family: &str,
) -> FilterSql {
    let mut conditions = Vec::new();
    let mut params = Vec::new();
    if let Some(root) = filter.command_root.filter(|value| !value.is_empty()) {
        conditions.push(format!("({command_root}) = ?"));
        params.push(root.to_string());
    }
    if let Some(command) = filter.command.filter(|value| !value.is_empty()) {
        conditions.push(format!("({command_family}) = ?"));
        params.push(command.to_string());
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    FilterSql {
        where_clause,
        params,
    }
}

pub fn largest_invocations(sort: SavingsSort, limit: usize) -> Result<Vec<InvocationInsight>> {
    largest_invocations_filtered(sort, limit, CommandFilter::default())
}

pub fn largest_invocations_filtered(
    sort: SavingsSort,
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<InvocationInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_invocations")? {
        return Ok(Vec::new());
    }
    let process = process_query_expression(&connection, None)?;
    let command_family = command_family_query_expression(&connection, None)?;
    let argv_json = argv_json_query_expression(&connection)?;
    let command_shape = optional_text_column_query_expression(&connection, None, "command_shape")?;
    let command_shape_hash =
        optional_text_column_query_expression(&connection, None, "command_shape_hash")?;
    let source = source_query_expression(&connection, None)?;
    let thread_id = optional_text_column_query_expression(&connection, None, "thread_id")?;
    let plan_title = optional_text_column_query_expression(&connection, None, "plan_title")?;
    let plan_project_id =
        optional_text_column_query_expression(&connection, None, "plan_project_id")?;
    let plan_folder_id =
        optional_text_column_query_expression(&connection, None, "plan_folder_id")?;
    let cwd = optional_text_column_query_expression(&connection, None, "cwd")?;
    let git_root = optional_text_column_query_expression(&connection, None, "git_root")?;
    let binary_version =
        optional_text_column_query_expression(&connection, None, "binary_version")?;
    let binary_revision =
        optional_text_column_query_expression(&connection, None, "binary_revision")?;
    let binary_fingerprint =
        optional_text_column_query_expression(&connection, None, "binary_fingerprint")?;
    let emitted_response_preview =
        optional_text_column_query_expression(&connection, None, "emitted_response_preview")?;
    let raw_response_preview =
        optional_text_column_query_expression(&connection, None, "raw_response_preview")?;
    let expansion = ExpansionSql::load(&connection, None)?;
    let filter_sql = invocation_filter_sql(&connection, filter)?;

    let sql = format!(
        "
        SELECT id, occurred_at_ms, {process} AS process,
               {command_family} AS command_family, command, {argv_json} AS argv_json,
               {command_shape} AS command_shape, {command_shape_hash} AS command_shape_hash,
               {source} AS source, {thread_id} AS thread_id, {plan_title} AS plan_title,
               {plan_project_id} AS plan_project_id, {plan_folder_id} AS plan_folder_id,
               {cwd} AS cwd, {git_root} AS git_root,
               {binary_version} AS binary_version, {binary_revision} AS binary_revision,
               {binary_fingerprint} AS binary_fingerprint,
               {emitted_response_preview} AS emitted_response_preview,
               {raw_response_preview} AS raw_response_preview, exit_code,
               raw_bytes, raw_chars, raw_lines, raw_tokens,
               emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
               saved_bytes, saved_chars, saved_lines, saved_tokens,
               {} AS expanded_bytes, {} AS expanded_chars,
               {} AS expanded_lines, {} AS expanded_tokens,
               {} AS expansion_reason,
               savings_ratio, compression_ratio
        FROM command_invocations
        {}
        ORDER BY {} DESC, occurred_at_ms DESC, id DESC
        LIMIT ?
        ",
        expansion.bytes,
        expansion.chars,
        expansion.lines,
        expansion.tokens,
        expansion.reason,
        filter_sql.where_clause,
        savings_order_column(sort)
    );
    let mut statement = connection.prepare(&sql)?;
    let mut params = filter_sql.params;
    params.push(bounded_limit(limit).to_string());
    let rows = statement.query_map(params_from_iter(params.iter()), invocation_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn recent_invocations(limit: usize) -> Result<Vec<InvocationInsight>> {
    recent_invocations_filtered(limit, CommandFilter::default())
}

pub fn recent_invocations_filtered(
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<InvocationInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_invocations")? {
        return Ok(Vec::new());
    }
    let process = process_query_expression(&connection, None)?;
    let command_family = command_family_query_expression(&connection, None)?;
    let argv_json = argv_json_query_expression(&connection)?;
    let command_shape = optional_text_column_query_expression(&connection, None, "command_shape")?;
    let command_shape_hash =
        optional_text_column_query_expression(&connection, None, "command_shape_hash")?;
    let source = source_query_expression(&connection, None)?;
    let thread_id = optional_text_column_query_expression(&connection, None, "thread_id")?;
    let plan_title = optional_text_column_query_expression(&connection, None, "plan_title")?;
    let plan_project_id =
        optional_text_column_query_expression(&connection, None, "plan_project_id")?;
    let plan_folder_id =
        optional_text_column_query_expression(&connection, None, "plan_folder_id")?;
    let cwd = optional_text_column_query_expression(&connection, None, "cwd")?;
    let git_root = optional_text_column_query_expression(&connection, None, "git_root")?;
    let binary_version =
        optional_text_column_query_expression(&connection, None, "binary_version")?;
    let binary_revision =
        optional_text_column_query_expression(&connection, None, "binary_revision")?;
    let binary_fingerprint =
        optional_text_column_query_expression(&connection, None, "binary_fingerprint")?;
    let emitted_response_preview =
        optional_text_column_query_expression(&connection, None, "emitted_response_preview")?;
    let raw_response_preview =
        optional_text_column_query_expression(&connection, None, "raw_response_preview")?;
    let expansion = ExpansionSql::load(&connection, None)?;
    let filter_sql = invocation_filter_sql(&connection, filter)?;

    let sql = format!(
        "
        SELECT id, occurred_at_ms, {process} AS process,
               {command_family} AS command_family, command, {argv_json} AS argv_json,
               {command_shape} AS command_shape, {command_shape_hash} AS command_shape_hash,
               {source} AS source, {thread_id} AS thread_id, {plan_title} AS plan_title,
               {plan_project_id} AS plan_project_id, {plan_folder_id} AS plan_folder_id,
               {cwd} AS cwd, {git_root} AS git_root,
               {binary_version} AS binary_version, {binary_revision} AS binary_revision,
               {binary_fingerprint} AS binary_fingerprint,
               {emitted_response_preview} AS emitted_response_preview,
               {raw_response_preview} AS raw_response_preview, exit_code,
               raw_bytes, raw_chars, raw_lines, raw_tokens,
               emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
               saved_bytes, saved_chars, saved_lines, saved_tokens,
               {} AS expanded_bytes, {} AS expanded_chars,
               {} AS expanded_lines, {} AS expanded_tokens,
               {} AS expansion_reason,
               savings_ratio, compression_ratio
        FROM command_invocations
        {}
        ORDER BY occurred_at_ms DESC, id DESC
        LIMIT ?
        ",
        expansion.bytes,
        expansion.chars,
        expansion.lines,
        expansion.tokens,
        expansion.reason,
        filter_sql.where_clause,
    );
    let mut statement = connection.prepare(&sql)?;
    let mut params = filter_sql.params;
    params.push(bounded_limit(limit).to_string());
    let rows = statement.query_map(params_from_iter(params.iter()), invocation_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn expansion_invocations(limit: usize) -> Result<Vec<InvocationInsight>> {
    expansion_invocations_filtered(limit, CommandFilter::default())
}

pub fn expansion_invocations_filtered(
    limit: usize,
    filter: CommandFilter<'_>,
) -> Result<Vec<InvocationInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_invocations")? {
        return Ok(Vec::new());
    }
    let process = process_query_expression(&connection, None)?;
    let command_family = command_family_query_expression(&connection, None)?;
    let argv_json = argv_json_query_expression(&connection)?;
    let command_shape = optional_text_column_query_expression(&connection, None, "command_shape")?;
    let command_shape_hash =
        optional_text_column_query_expression(&connection, None, "command_shape_hash")?;
    let source = source_query_expression(&connection, None)?;
    let thread_id = optional_text_column_query_expression(&connection, None, "thread_id")?;
    let plan_title = optional_text_column_query_expression(&connection, None, "plan_title")?;
    let plan_project_id =
        optional_text_column_query_expression(&connection, None, "plan_project_id")?;
    let plan_folder_id =
        optional_text_column_query_expression(&connection, None, "plan_folder_id")?;
    let cwd = optional_text_column_query_expression(&connection, None, "cwd")?;
    let git_root = optional_text_column_query_expression(&connection, None, "git_root")?;
    let binary_version =
        optional_text_column_query_expression(&connection, None, "binary_version")?;
    let binary_revision =
        optional_text_column_query_expression(&connection, None, "binary_revision")?;
    let binary_fingerprint =
        optional_text_column_query_expression(&connection, None, "binary_fingerprint")?;
    let emitted_response_preview =
        optional_text_column_query_expression(&connection, None, "emitted_response_preview")?;
    let raw_response_preview =
        optional_text_column_query_expression(&connection, None, "raw_response_preview")?;
    let expansion = ExpansionSql::load(&connection, None)?;
    let filter_sql = invocation_filter_sql(&connection, filter)?;
    let where_clause = filter_sql.with_condition(&format!("({})", expansion.predicate()));

    let sql = format!(
        "
        SELECT id, occurred_at_ms, {process} AS process,
               {command_family} AS command_family, command, {argv_json} AS argv_json,
               {command_shape} AS command_shape, {command_shape_hash} AS command_shape_hash,
               {source} AS source, {thread_id} AS thread_id, {plan_title} AS plan_title,
               {plan_project_id} AS plan_project_id, {plan_folder_id} AS plan_folder_id,
               {cwd} AS cwd, {git_root} AS git_root,
               {binary_version} AS binary_version, {binary_revision} AS binary_revision,
               {binary_fingerprint} AS binary_fingerprint,
               {emitted_response_preview} AS emitted_response_preview,
               {raw_response_preview} AS raw_response_preview, exit_code,
               raw_bytes, raw_chars, raw_lines, raw_tokens,
               emitted_bytes, emitted_chars, emitted_lines, emitted_tokens,
               saved_bytes, saved_chars, saved_lines, saved_tokens,
               {} AS expanded_bytes, {} AS expanded_chars,
               {} AS expanded_lines, {} AS expanded_tokens,
               {} AS expansion_reason,
               savings_ratio, compression_ratio
        FROM command_invocations
        {where_clause}
        ORDER BY expanded_tokens DESC, expanded_chars DESC, occurred_at_ms DESC, id DESC
        LIMIT ?
        ",
        expansion.bytes, expansion.chars, expansion.lines, expansion.tokens, expansion.reason,
    );
    let mut statement = connection.prepare(&sql)?;
    let mut params = filter_sql.params;
    params.push(bounded_limit(limit).to_string());
    let rows = statement.query_map(params_from_iter(params.iter()), invocation_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn invocation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InvocationInsight> {
    Ok(InvocationInsight {
        id: row.get(0)?,
        occurred_at_ms: row.get(1)?,
        process: row.get(2)?,
        command_family: row.get(3)?,
        command: row.get(4)?,
        argv_json: row.get(5)?,
        command_shape: row.get(6)?,
        command_shape_hash: row.get(7)?,
        source: row.get(8)?,
        thread_id: row.get(9)?,
        plan_title: row.get(10)?,
        plan_project_id: row.get(11)?,
        plan_folder_id: row.get(12)?,
        cwd: row.get(13)?,
        git_root: row.get(14)?,
        binary_version: row.get(15)?,
        binary_revision: row.get(16)?,
        binary_fingerprint: row.get(17)?,
        emitted_response_preview: row.get(18)?,
        raw_response_preview: row.get(19)?,
        exit_code: row.get(20)?,
        raw: TextMetrics {
            bytes: row.get(21)?,
            chars: row.get(22)?,
            lines: row.get(23)?,
            tokens: row.get(24)?,
        },
        emitted: TextMetrics {
            bytes: row.get(25)?,
            chars: row.get(26)?,
            lines: row.get(27)?,
            tokens: row.get(28)?,
        },
        saved: TextMetrics {
            bytes: row.get(29)?,
            chars: row.get(30)?,
            lines: row.get(31)?,
            tokens: row.get(32)?,
        },
        expanded: TextMetrics {
            bytes: row.get(33)?,
            chars: row.get(34)?,
            lines: row.get(35)?,
            tokens: row.get(36)?,
        },
        expansion_reason: row.get(37)?,
        savings_ratio: row.get(38)?,
        compression_ratio: row.get(39)?,
    })
}

pub fn daily_totals(limit: usize) -> Result<Vec<DailyInsight>> {
    daily_totals_filtered(limit, CommandFilter::default())
}

pub fn daily_totals_filtered(limit: usize, filter: CommandFilter<'_>) -> Result<Vec<DailyInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_invocations")? {
        return Ok(Vec::new());
    }
    let classified_failure = classified_failure_sql(&connection, None)?;
    let expansion = ExpansionSql::load(&connection, None)?;
    let filter_sql = invocation_filter_sql(&connection, filter)?;

    let sql = format!(
        "
        SELECT strftime('%Y-%m-%d', occurred_at_ms / 1000, 'unixepoch') AS day,
               COUNT(*) AS invocations,
               SUM({classified_failure}) AS failures,
               SUM(CASE WHEN {} THEN 1 ELSE 0 END) AS expansions,
               SUM(saved_bytes), SUM(saved_chars), SUM(saved_lines), SUM(saved_tokens),
               SUM({}), SUM({}), SUM({}), SUM({})
        FROM command_invocations
        {}
        GROUP BY day
        ORDER BY day DESC
        LIMIT ?
        ",
        expansion.predicate(),
        expansion.bytes,
        expansion.chars,
        expansion.lines,
        expansion.tokens,
        filter_sql.where_clause,
    );
    let mut statement = connection.prepare(&sql)?;
    let mut params = filter_sql.params;
    params.push(bounded_limit(limit).to_string());
    let rows = statement.query_map(params_from_iter(params.iter()), |row| {
        Ok(DailyInsight {
            day: row.get(0)?,
            invocations: row.get(1)?,
            failures: row.get(2)?,
            expansions: row.get(3)?,
            saved: TextMetrics {
                bytes: row.get(4)?,
                chars: row.get(5)?,
                lines: row.get(6)?,
                tokens: row.get(7)?,
            },
            expanded: TextMetrics {
                bytes: row.get(8)?,
                chars: row.get(9)?,
                lines: row.get(10)?,
                tokens: row.get(11)?,
            },
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn recent_failure_details(limit: usize) -> Result<Vec<FailureDetailInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_failures")? {
        return Ok(Vec::new());
    }

    let invocation_id = nullable_integer_column_query_expression_for_table(
        &connection,
        "command_failures",
        None,
        "invocation_id",
    )?;
    let artifact_ref = optional_text_column_query_expression_for_table(
        &connection,
        "command_failures",
        None,
        "artifact_ref",
    )?;
    let mut statement = connection.prepare(&format!(
        "
        SELECT id, {invocation_id}, occurred_at_ms, command_family, command_line, exit_code,
               cx_response, raw_source, raw_response,
               {artifact_ref}
        FROM command_failures
        ORDER BY occurred_at_ms DESC, id DESC
        LIMIT ?1
        ",
    ))?;
    let rows = statement.query_map([bounded_limit(limit)], |row| {
        Ok(FailureDetailInsight {
            id: row.get(0)?,
            invocation_id: row.get(1)?,
            occurred_at_ms: row.get(2)?,
            command_family: row.get(3)?,
            command_line: row.get(4)?,
            exit_code: row.get(5)?,
            cx_response: row.get(6)?,
            raw_source: row.get(7)?,
            raw_response: row.get(8)?,
            artifact_ref: row.get(9)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn overall_totals() -> Result<OverallInsight> {
    overall_totals_from_invocations(CommandFilter::default())
}

pub fn overall_totals_filtered(filter: CommandFilter<'_>) -> Result<OverallInsight> {
    overall_totals_from_invocations(filter)
}

pub fn savings_distribution() -> Result<SavingsDistributionInsight> {
    savings_distribution_filtered(CommandFilter::default())
}

pub fn savings_distribution_filtered(
    filter: CommandFilter<'_>,
) -> Result<SavingsDistributionInsight> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(SavingsDistributionInsight::default());
    };
    if !table_exists(&connection, "command_invocations")? {
        return Ok(SavingsDistributionInsight::default());
    }
    if filter.is_empty() && database_index_exists(&connection, SAVED_TOKENS_INDEX)? {
        return indexed_savings_distribution(&connection);
    }
    ranked_savings_distribution(&connection, invocation_filter_sql(&connection, filter)?)
}

fn indexed_savings_distribution(connection: &Connection) -> Result<SavingsDistributionInsight> {
    let (invocations, saving_invocations, total_saved_tokens, largest_saved_tokens) = connection
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN saved_tokens > 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(saved_tokens), 0),
                    COALESCE(MAX(saved_tokens), 0)
             FROM command_invocations",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    if invocations == 0 {
        return Ok(SavingsDistributionInsight::default());
    }
    let top_ten_saved_tokens = connection.query_row(
        "SELECT COALESCE(SUM(saved_tokens), 0)
         FROM (
             SELECT saved_tokens
             FROM command_invocations
             ORDER BY saved_tokens DESC, id DESC
             LIMIT 10
         )",
        [],
        |row| row.get(0),
    )?;

    Ok(SavingsDistributionInsight {
        invocations,
        saving_invocations,
        total_saved_tokens,
        all_p50_saved_tokens: indexed_nearest_rank(connection, invocations, 50, false)?,
        all_p95_saved_tokens: indexed_nearest_rank(connection, invocations, 95, false)?,
        all_p99_saved_tokens: indexed_nearest_rank(connection, invocations, 99, false)?,
        saving_p50_saved_tokens: indexed_nearest_rank(connection, saving_invocations, 50, true)?,
        saving_p95_saved_tokens: indexed_nearest_rank(connection, saving_invocations, 95, true)?,
        saving_p99_saved_tokens: indexed_nearest_rank(connection, saving_invocations, 99, true)?,
        largest_saved_tokens,
        top_ten_saved_tokens,
    })
}

fn indexed_nearest_rank(
    connection: &Connection,
    count: u64,
    percentile: u64,
    saving_only: bool,
) -> Result<u64> {
    if count == 0 {
        return Ok(0);
    }
    let rank = count.saturating_mul(percentile).saturating_add(99) / 100;
    let offset = i64::try_from(rank.saturating_sub(1))?;
    let sql = if saving_only {
        "SELECT saved_tokens
         FROM command_invocations
         WHERE saved_tokens > 0
         ORDER BY saved_tokens ASC, id ASC
         LIMIT 1 OFFSET ?1"
    } else {
        "SELECT saved_tokens
         FROM command_invocations
         ORDER BY saved_tokens ASC, id ASC
         LIMIT 1 OFFSET ?1"
    };
    connection
        .query_row(sql, [offset], |row| row.get(0))
        .map_err(Into::into)
}

fn database_index_exists(connection: &Connection, name: &str) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1
             )",
            [name],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn ranked_savings_distribution(
    connection: &Connection,
    filter_sql: FilterSql,
) -> Result<SavingsDistributionInsight> {
    let sql = format!(
        "
        WITH ranked AS (
            SELECT saved_tokens,
                   ROW_NUMBER() OVER (ORDER BY saved_tokens ASC, id ASC) AS value_rank,
                   COUNT(*) OVER () AS invocations,
                   SUM(CASE WHEN saved_tokens > 0 THEN 1 ELSE 0 END)
                       OVER () AS saving_invocations,
                   SUM(saved_tokens) OVER () AS total_saved_tokens
            FROM command_invocations
            {}
        )
        SELECT COALESCE(MAX(invocations), 0),
               COALESCE(MAX(saving_invocations), 0),
               COALESCE(MAX(total_saved_tokens), 0),
               COALESCE(MAX(CASE
                   WHEN value_rank = ((invocations * 50 + 99) / 100) THEN saved_tokens
               END), 0),
               COALESCE(MAX(CASE
                   WHEN value_rank = ((invocations * 95 + 99) / 100) THEN saved_tokens
               END), 0),
               COALESCE(MAX(CASE
                   WHEN value_rank = ((invocations * 99 + 99) / 100) THEN saved_tokens
               END), 0),
               COALESCE(MAX(CASE
                   WHEN saved_tokens > 0
                    AND value_rank - (invocations - saving_invocations)
                        = ((saving_invocations * 50 + 99) / 100)
                   THEN saved_tokens
               END), 0),
               COALESCE(MAX(CASE
                   WHEN saved_tokens > 0
                    AND value_rank - (invocations - saving_invocations)
                        = ((saving_invocations * 95 + 99) / 100)
                   THEN saved_tokens
               END), 0),
               COALESCE(MAX(CASE
                   WHEN saved_tokens > 0
                    AND value_rank - (invocations - saving_invocations)
                        = ((saving_invocations * 99 + 99) / 100)
                   THEN saved_tokens
               END), 0),
               COALESCE(MAX(saved_tokens), 0),
               COALESCE(SUM(CASE
                   WHEN value_rank > invocations - 10 THEN saved_tokens ELSE 0
               END), 0)
        FROM ranked
        ",
        filter_sql.where_clause,
    );
    connection
        .query_row(&sql, params_from_iter(filter_sql.params.iter()), |row| {
            Ok(SavingsDistributionInsight {
                invocations: row.get(0)?,
                saving_invocations: row.get(1)?,
                total_saved_tokens: row.get(2)?,
                all_p50_saved_tokens: row.get(3)?,
                all_p95_saved_tokens: row.get(4)?,
                all_p99_saved_tokens: row.get(5)?,
                saving_p50_saved_tokens: row.get(6)?,
                saving_p95_saved_tokens: row.get(7)?,
                saving_p99_saved_tokens: row.get(8)?,
                largest_saved_tokens: row.get(9)?,
                top_ten_saved_tokens: row.get(10)?,
            })
        })
        .map_err(Into::into)
}

fn overall_totals_from_invocations(filter: CommandFilter<'_>) -> Result<OverallInsight> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(OverallInsight::default());
    };
    if !table_exists(&connection, "command_invocations")? {
        return Ok(OverallInsight::default());
    }
    let classified_failure = classified_failure_sql(&connection, None)?;
    let expansion = ExpansionSql::load(&connection, None)?;
    let filter_sql = invocation_filter_sql(&connection, filter)?;

    let sql = format!(
        "
            SELECT COALESCE(COUNT(*), 0),
                   COALESCE(SUM({classified_failure}), 0),
                   COALESCE(SUM(CASE WHEN {} THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(raw_bytes), 0),
                   COALESCE(SUM(raw_chars), 0),
                   COALESCE(SUM(raw_lines), 0),
                   COALESCE(SUM(raw_tokens), 0),
                   COALESCE(SUM(emitted_bytes), 0),
                   COALESCE(SUM(emitted_chars), 0),
                   COALESCE(SUM(emitted_lines), 0),
                   COALESCE(SUM(emitted_tokens), 0),
                   COALESCE(SUM(saved_bytes), 0),
                   COALESCE(SUM(saved_chars), 0),
                   COALESCE(SUM(saved_lines), 0),
                   COALESCE(SUM(saved_tokens), 0),
                   COALESCE(SUM({}), 0),
                   COALESCE(SUM({}), 0),
                   COALESCE(SUM({}), 0),
                   COALESCE(SUM({}), 0)
            FROM command_invocations
            {}
        ",
        expansion.predicate(),
        expansion.bytes,
        expansion.chars,
        expansion.lines,
        expansion.tokens,
        filter_sql.where_clause
    );
    connection
        .query_row(&sql, params_from_iter(filter_sql.params.iter()), |row| {
            Ok(OverallInsight {
                invocations: row.get(0)?,
                failures: row.get(1)?,
                expansions: row.get(2)?,
                raw: TextMetrics {
                    bytes: row.get(3)?,
                    chars: row.get(4)?,
                    lines: row.get(5)?,
                    tokens: row.get(6)?,
                },
                emitted: TextMetrics {
                    bytes: row.get(7)?,
                    chars: row.get(8)?,
                    lines: row.get(9)?,
                    tokens: row.get(10)?,
                },
                saved: TextMetrics {
                    bytes: row.get(11)?,
                    chars: row.get(12)?,
                    lines: row.get(13)?,
                    tokens: row.get(14)?,
                },
                expanded: TextMetrics {
                    bytes: row.get(15)?,
                    chars: row.get(16)?,
                    lines: row.get(17)?,
                    tokens: row.get(18)?,
                },
            })
        })
        .map_err(Into::into)
}

struct ExpansionSql {
    bytes: String,
    chars: String,
    lines: String,
    tokens: String,
    reason: String,
}

impl ExpansionSql {
    fn load(connection: &Connection, alias: Option<&str>) -> Result<Self> {
        Ok(Self {
            bytes: optional_integer_column_query_expression(connection, alias, "expanded_bytes")?,
            chars: optional_integer_column_query_expression(connection, alias, "expanded_chars")?,
            lines: optional_integer_column_query_expression(connection, alias, "expanded_lines")?,
            tokens: optional_integer_column_query_expression(connection, alias, "expanded_tokens")?,
            reason: optional_text_column_query_expression(connection, alias, "expansion_reason")?,
        })
    }

    fn predicate(&self) -> String {
        format!(
            "{} > 0 OR {} > 0 OR {} > 0 OR {} > 0",
            self.bytes, self.chars, self.lines, self.tokens
        )
    }
}

pub fn insights_database_path() -> Result<PathBuf> {
    insights_db_file()
}

struct FilterSql {
    where_clause: String,
    params: Vec<String>,
}

impl FilterSql {
    fn with_condition(&self, condition: &str) -> String {
        if self.where_clause.is_empty() {
            format!("WHERE {condition}")
        } else {
            format!("{} AND {condition}", self.where_clause)
        }
    }
}

fn invocation_filter_sql(connection: &Connection, filter: CommandFilter<'_>) -> Result<FilterSql> {
    invocation_filter_sql_with_alias(connection, filter, None)
}

fn invocation_filter_sql_with_alias(
    connection: &Connection,
    filter: CommandFilter<'_>,
    alias: Option<&str>,
) -> Result<FilterSql> {
    let mut conditions = Vec::new();
    let mut params = Vec::new();
    if let Some(command_root) = filter.command_root.filter(|value| !value.is_empty()) {
        let process_expression = process_query_expression(connection, alias)?;
        conditions.push(format!("({process_expression}) = ?"));
        params.push(command_root.to_string());
    }
    if let Some(command) = filter.command.filter(|value| !value.is_empty()) {
        let command_family_expression = command_family_query_expression(connection, alias)?;
        conditions.push(format!("({command_family_expression}) = ?"));
        params.push(command.to_string());
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

pub fn command_root(command: &str) -> &str {
    command
        .split_whitespace()
        .next()
        .filter(|root| !root.is_empty())
        .unwrap_or(command)
}

fn command_total_order_column(sort: CommandTotalSort) -> &'static str {
    match sort {
        CommandTotalSort::Tokens => "saved_tokens",
        CommandTotalSort::Chars => "saved_chars",
        CommandTotalSort::Lines => "saved_lines",
        CommandTotalSort::Invocations => "invocations",
        CommandTotalSort::Failures => "failures",
    }
}

fn savings_order_column(sort: SavingsSort) -> &'static str {
    match sort {
        SavingsSort::Tokens => "saved_tokens",
        SavingsSort::Chars => "saved_chars",
        SavingsSort::Lines => "saved_lines",
    }
}

pub(super) fn bounded_limit(limit: usize) -> i64 {
    limit.clamp(1, 1_000) as i64
}
