use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags};

use super::TextMetrics;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivePathInsight {
    pub path: PathBuf,
    pub rows: u64,
    pub distinct_invocations: u64,
    pub first_utc: String,
    pub last_utc: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArchiveDedupeInsight {
    pub raw_archive_rows: u64,
    pub invocation_id_rows: u64,
    pub canonical_rows: u64,
    pub archive_duplicate_rows: u64,
    pub import_fanout_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArchiveOverallInsight {
    pub invocations: u64,
    pub nonzero: u64,
    pub empty_argv: u64,
    pub raw: TextMetrics,
    pub emitted: TextMetrics,
    pub saved: TextMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveSupportClassInsight {
    pub support_class: String,
    pub invocations: u64,
    pub raw_tokens: u64,
    pub saved_tokens: u64,
    pub nonzero: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCommandTotalInsight {
    pub command_family: String,
    pub process: String,
    pub invocations: u64,
    pub nonzero: u64,
    pub raw_tokens: u64,
    pub emitted_tokens: u64,
    pub saved_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveSourceTotalInsight {
    pub name: String,
    pub invocations: u64,
    pub raw_tokens: u64,
    pub saved_tokens: u64,
    pub nonzero: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCommandQualityInsight {
    pub command_family: String,
    pub invocations: u64,
    pub empty_argv: u64,
    pub family_only_command: u64,
    pub empty_shape: u64,
    pub family_only_shape: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArchiveCoverageInsight {
    pub rows: u64,
    pub distinct_rows: u64,
    pub artifact_rows: u64,
    pub preview_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveSummary {
    pub archives: Vec<ArchivePathInsight>,
    pub dedupe: ArchiveDedupeInsight,
    pub overall: ArchiveOverallInsight,
    pub support_classes: Vec<ArchiveSupportClassInsight>,
    pub top_saved_commands: Vec<ArchiveCommandTotalInsight>,
    pub top_opportunity_commands: Vec<ArchiveCommandTotalInsight>,
    pub source_machines: Vec<ArchiveSourceTotalInsight>,
    pub git_roots: Vec<ArchiveSourceTotalInsight>,
    pub command_quality: Vec<ArchiveCommandQualityInsight>,
    pub expansion: Vec<ArchiveCommandTotalInsight>,
    pub failure_coverage: ArchiveCoverageInsight,
    pub report_coverage: ArchiveCoverageInsight,
}

pub fn archive_summary(paths: &[PathBuf], limit: usize) -> Result<ArchiveSummary> {
    let paths = normalized_archive_paths(paths)?;
    let connection = open_archive_connection(&paths)?;
    prepare_archive_tables(&connection, &paths)?;
    let limit = limit.clamp(1, 100);
    Ok(ArchiveSummary {
        archives: archive_path_insights(&connection, &paths)?,
        dedupe: archive_dedupe(&connection)?,
        overall: archive_overall(&connection)?,
        support_classes: archive_support_classes(&connection)?,
        top_saved_commands: archive_command_totals(
            &connection,
            "total_saved_tokens DESC, total_raw_tokens DESC, command_family ASC",
            "total_saved_tokens > 0",
            limit,
        )?,
        top_opportunity_commands: archive_command_totals(
            &connection,
            "total_raw_tokens DESC, invocations DESC, command_family ASC",
            "total_saved_tokens = 0 AND total_raw_tokens > 0",
            limit,
        )?,
        source_machines: archive_source_totals(&connection, "source_machine", limit)?,
        git_roots: archive_source_totals(&connection, "git_root", limit)?,
        command_quality: archive_command_quality(&connection, limit)?,
        expansion: archive_command_totals(
            &connection,
            "total_emitted_tokens - total_raw_tokens DESC, command_family ASC",
            "total_emitted_tokens > total_raw_tokens",
            limit,
        )?,
        failure_coverage: archive_failure_coverage(&connection, &paths)?,
        report_coverage: archive_report_coverage(&connection, &paths)?,
    })
}

fn normalized_archive_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        bail!("archive-summary requires at least one --archive path");
    }
    let mut output = Vec::with_capacity(paths.len());
    for path in paths {
        if !path.is_file() {
            bail!("archive database does not exist: {}", path.display());
        }
        output.push(path.clone());
    }
    Ok(output)
}

fn open_archive_connection(paths: &[PathBuf]) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        &paths[0],
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("failed to open archive {}", paths[0].display()))?;
    for (index, path) in paths.iter().enumerate().skip(1) {
        let alias = archive_alias(index);
        connection.execute(
            &format!("ATTACH DATABASE ?1 AS {alias}"),
            params![path.to_string_lossy().as_ref()],
        )?;
    }
    Ok(connection)
}

fn prepare_archive_tables(connection: &Connection, paths: &[PathBuf]) -> Result<()> {
    for (index, path) in paths.iter().enumerate() {
        let schema = archive_schema(index);
        if !archive_table_exists(connection, &schema, "command_invocations")? {
            bail!("archive {} is missing command_invocations", path.display());
        }
    }
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS cx_archive_all_rows;
        DROP TABLE IF EXISTS cx_archive_by_invocation;
        DROP TABLE IF EXISTS cx_archive_canonical;
        ",
    )?;
    let union_sql = (0..paths.len())
        .map(|index| archive_invocation_select(connection, index))
        .collect::<Result<Vec<_>>>()?
        .join(" UNION ALL ");
    connection.execute_batch(&format!(
        "
        CREATE TEMP TABLE cx_archive_all_rows AS {union_sql};
        CREATE TEMP TABLE cx_archive_by_invocation AS
            SELECT *,
                   ROW_NUMBER() OVER (PARTITION BY invocation_id ORDER BY archive) AS rn_invocation
            FROM cx_archive_all_rows;
        CREATE TEMP TABLE cx_archive_canonical AS
            SELECT *,
                   COALESCE(NULLIF(json_extract(metadata_json, '$.process'), ''), {legacy_process}) AS process,
                   CASE WHEN command_family LIKE 'passthrough %' THEN 'passthrough' ELSE 'official' END AS support_class,
                   COALESCE(json_extract(metrics_json, '$.rawBytes'), 0) AS raw_bytes,
                   COALESCE(json_extract(metrics_json, '$.rawChars'), 0) AS raw_chars,
                   COALESCE(json_extract(metrics_json, '$.rawLines'), 0) AS raw_lines,
                   COALESCE(json_extract(metrics_json, '$.rawTokens'), 0) AS raw_tokens,
                   COALESCE(json_extract(metrics_json, '$.emittedBytes'), 0) AS emitted_bytes,
                   COALESCE(json_extract(metrics_json, '$.emittedChars'), 0) AS emitted_chars,
                   COALESCE(json_extract(metrics_json, '$.emittedLines'), 0) AS emitted_lines,
                   COALESCE(json_extract(metrics_json, '$.emittedTokens'), 0) AS emitted_tokens,
                   COALESCE(json_extract(metrics_json, '$.savedBytes'), 0) AS saved_bytes,
                   COALESCE(json_extract(metrics_json, '$.savedChars'), 0) AS saved_chars,
                   COALESCE(json_extract(metrics_json, '$.savedLines'), 0) AS saved_lines,
                   COALESCE(json_extract(metrics_json, '$.savedTokens'), 0) AS saved_tokens
            FROM (
                SELECT *,
                       ROW_NUMBER() OVER (
                           PARTITION BY occurred_at_ms,
                                        COALESCE(cwd, ''),
                                        COALESCE(git_root, ''),
                                        command_family,
                                        command,
                                        exit_code,
                                        COALESCE(json_extract(metadata_json, '$.sourceId'), ''),
                                        metrics_json
                           ORDER BY CASE
                                        WHEN json_extract(metadata_json, '$.contextSource') = 'sender_fallback' THEN 0
                                        WHEN json_extract(metadata_json, '$.source') = 'cx_insights_remote_sender' THEN 1
                                        ELSE 2
                                    END,
                                    archive,
                                    source_machine
                       ) AS rn_canonical
                FROM cx_archive_by_invocation
                WHERE rn_invocation = 1
            )
            WHERE rn_canonical = 1;
        ",
        legacy_process = legacy_process_sql("command_family"),
    ))?;
    Ok(())
}

fn archive_invocation_select(connection: &Connection, index: usize) -> Result<String> {
    let schema = archive_schema(index);
    let archive = archive_alias(index);
    let command_shape =
        archive_command_shape_expression(connection, &schema, "command_invocations")?;
    let command_shape_hash =
        archive_command_shape_hash_expression(connection, &schema, "command_invocations")?;
    Ok(format!(
        "
        SELECT '{archive}' AS archive,
               invocation_id,
               occurred_at_ms,
               COALESCE(source_machine, '') AS source_machine,
               COALESCE(cwd, '') AS cwd,
               COALESCE(git_root, '') AS git_root,
               COALESCE(command_family, command, '') AS command_family,
               COALESCE(command, command_family, '') AS command,
               COALESCE(argv_json, '[]') AS argv_json,
               {command_shape} AS command_shape,
               {command_shape_hash} AS command_shape_hash,
               exit_code,
               COALESCE(metrics_json, '{{}}') AS metrics_json,
               COALESCE(metadata_json, '{{}}') AS metadata_json
        FROM {schema}.command_invocations
        "
    ))
}

fn archive_path_insights(
    connection: &Connection,
    paths: &[PathBuf],
) -> Result<Vec<ArchivePathInsight>> {
    let mut output = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let archive = archive_alias(index);
        output.push(connection.query_row(
            "
            SELECT COUNT(*),
                   COUNT(DISTINCT invocation_id),
                   COALESCE(MIN(datetime(occurred_at_ms / 1000, 'unixepoch')), ''),
                   COALESCE(MAX(datetime(occurred_at_ms / 1000, 'unixepoch')), '')
            FROM cx_archive_all_rows
            WHERE archive = ?1
            ",
            [archive],
            |row| {
                Ok(ArchivePathInsight {
                    path: path.clone(),
                    rows: row.get(0)?,
                    distinct_invocations: row.get(1)?,
                    first_utc: row.get(2)?,
                    last_utc: row.get(3)?,
                })
            },
        )?);
    }
    Ok(output)
}

fn archive_dedupe(connection: &Connection) -> Result<ArchiveDedupeInsight> {
    connection
        .query_row(
            "
            SELECT (SELECT COUNT(*) FROM cx_archive_all_rows),
                   (SELECT COUNT(*) FROM cx_archive_by_invocation WHERE rn_invocation = 1),
                   (SELECT COUNT(*) FROM cx_archive_canonical),
                   (SELECT COUNT(*) FROM cx_archive_all_rows)
                    - (SELECT COUNT(*) FROM cx_archive_by_invocation WHERE rn_invocation = 1),
                   (SELECT COUNT(*) FROM cx_archive_by_invocation WHERE rn_invocation = 1)
                    - (SELECT COUNT(*) FROM cx_archive_canonical)
            ",
            [],
            |row| {
                Ok(ArchiveDedupeInsight {
                    raw_archive_rows: row.get(0)?,
                    invocation_id_rows: row.get(1)?,
                    canonical_rows: row.get(2)?,
                    archive_duplicate_rows: row.get(3)?,
                    import_fanout_rows: row.get(4)?,
                })
            },
        )
        .map_err(Into::into)
}

fn archive_overall(connection: &Connection) -> Result<ArchiveOverallInsight> {
    connection
        .query_row(
            "
            SELECT COUNT(*),
                   COALESCE(SUM(CASE WHEN exit_code != 0 THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN argv_json = '[]' OR argv_json = '' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(raw_bytes), 0), COALESCE(SUM(raw_chars), 0),
                   COALESCE(SUM(raw_lines), 0), COALESCE(SUM(raw_tokens), 0),
                   COALESCE(SUM(emitted_bytes), 0), COALESCE(SUM(emitted_chars), 0),
                   COALESCE(SUM(emitted_lines), 0), COALESCE(SUM(emitted_tokens), 0),
                   COALESCE(SUM(saved_bytes), 0), COALESCE(SUM(saved_chars), 0),
                   COALESCE(SUM(saved_lines), 0), COALESCE(SUM(saved_tokens), 0)
            FROM cx_archive_canonical
            ",
            [],
            |row| {
                Ok(ArchiveOverallInsight {
                    invocations: row.get(0)?,
                    nonzero: row.get(1)?,
                    empty_argv: row.get(2)?,
                    raw: metrics_from_row(row, 3)?,
                    emitted: metrics_from_row(row, 7)?,
                    saved: metrics_from_row(row, 11)?,
                })
            },
        )
        .map_err(Into::into)
}

fn archive_support_classes(connection: &Connection) -> Result<Vec<ArchiveSupportClassInsight>> {
    let mut statement = connection.prepare(
        "
        SELECT support_class,
               COUNT(*) AS invocations,
               COALESCE(SUM(raw_tokens), 0) AS raw_tokens,
               COALESCE(SUM(saved_tokens), 0) AS saved_tokens,
               COALESCE(SUM(CASE WHEN exit_code != 0 THEN 1 ELSE 0 END), 0) AS nonzero
        FROM cx_archive_canonical
        GROUP BY support_class
        ORDER BY invocations DESC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(ArchiveSupportClassInsight {
            support_class: row.get(0)?,
            invocations: row.get(1)?,
            raw_tokens: row.get(2)?,
            saved_tokens: row.get(3)?,
            nonzero: row.get(4)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn archive_command_totals(
    connection: &Connection,
    order_by: &str,
    having: &str,
    limit: usize,
) -> Result<Vec<ArchiveCommandTotalInsight>> {
    let sql = format!(
        "
        SELECT command_family,
               process,
               COUNT(*) AS invocations,
               COALESCE(SUM(CASE WHEN exit_code != 0 THEN 1 ELSE 0 END), 0) AS nonzero,
               COALESCE(SUM(raw_tokens), 0) AS total_raw_tokens,
               COALESCE(SUM(emitted_tokens), 0) AS total_emitted_tokens,
               COALESCE(SUM(saved_tokens), 0) AS total_saved_tokens
        FROM cx_archive_canonical
        GROUP BY command_family, process
        HAVING {having}
        ORDER BY {order_by}
        LIMIT ?1
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([limit as i64], archive_command_total_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn archive_source_totals(
    connection: &Connection,
    column: &str,
    limit: usize,
) -> Result<Vec<ArchiveSourceTotalInsight>> {
    let sql = format!(
        "
        SELECT COALESCE(NULLIF({column}, ''), '(unknown)') AS name,
               COUNT(*) AS invocations,
               COALESCE(SUM(raw_tokens), 0) AS raw_tokens,
               COALESCE(SUM(saved_tokens), 0) AS saved_tokens,
               COALESCE(SUM(CASE WHEN exit_code != 0 THEN 1 ELSE 0 END), 0) AS nonzero
        FROM cx_archive_canonical
        GROUP BY name
        ORDER BY invocations DESC, raw_tokens DESC
        LIMIT ?1
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([limit as i64], |row| {
        Ok(ArchiveSourceTotalInsight {
            name: row.get(0)?,
            invocations: row.get(1)?,
            raw_tokens: row.get(2)?,
            saved_tokens: row.get(3)?,
            nonzero: row.get(4)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn archive_command_quality(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<ArchiveCommandQualityInsight>> {
    let mut statement = connection.prepare(
        "
        SELECT command_family,
               COUNT(*) AS invocations,
               COALESCE(SUM(CASE WHEN argv_json = '[]' OR argv_json = '' THEN 1 ELSE 0 END), 0) AS empty_argv,
               COALESCE(SUM(CASE WHEN command = command_family THEN 1 ELSE 0 END), 0) AS family_only_command,
               COALESCE(SUM(CASE WHEN command_shape = '' THEN 1 ELSE 0 END), 0) AS empty_shape,
               COALESCE(SUM(CASE WHEN command_shape = command_family THEN 1 ELSE 0 END), 0) AS family_only_shape
        FROM cx_archive_canonical
        GROUP BY command_family
        ORDER BY invocations DESC
        LIMIT ?1
        ",
    )?;
    let rows = statement.query_map([limit as i64], |row| {
        Ok(ArchiveCommandQualityInsight {
            command_family: row.get(0)?,
            invocations: row.get(1)?,
            empty_argv: row.get(2)?,
            family_only_command: row.get(3)?,
            empty_shape: row.get(4)?,
            family_only_shape: row.get(5)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn archive_failure_coverage(
    connection: &Connection,
    paths: &[PathBuf],
) -> Result<ArchiveCoverageInsight> {
    let selects = archive_failure_selects(connection, paths)?;
    if selects.is_empty() {
        return Ok(ArchiveCoverageInsight::default());
    }
    archive_coverage_from_union(connection, &selects.join(" UNION ALL "), "failure_id")
}

fn archive_report_coverage(
    connection: &Connection,
    paths: &[PathBuf],
) -> Result<ArchiveCoverageInsight> {
    let selects = archive_report_selects(connection, paths)?;
    if selects.is_empty() {
        return Ok(ArchiveCoverageInsight::default());
    }
    archive_coverage_from_union(connection, &selects.join(" UNION ALL "), "report_id")
}

fn archive_coverage_from_union(
    connection: &Connection,
    union_sql: &str,
    id_column: &str,
) -> Result<ArchiveCoverageInsight> {
    let sql = format!(
        "
        WITH rows AS ({union_sql})
        SELECT COUNT(*),
               COUNT(DISTINCT {id_column}),
               COALESCE(SUM(CASE WHEN artifact_ref != '' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN response_preview != '' THEN 1 ELSE 0 END), 0)
        FROM rows
        "
    );
    connection
        .query_row(&sql, [], |row| {
            Ok(ArchiveCoverageInsight {
                rows: row.get(0)?,
                distinct_rows: row.get(1)?,
                artifact_rows: row.get(2)?,
                preview_rows: row.get(3)?,
            })
        })
        .map_err(Into::into)
}

fn archive_failure_selects(connection: &Connection, paths: &[PathBuf]) -> Result<Vec<String>> {
    let mut selects = Vec::new();
    for index in 0..paths.len() {
        let schema = archive_schema(index);
        if !archive_table_exists(connection, &schema, "command_failures")? {
            continue;
        }
        let id = archive_text_column(connection, &schema, "command_failures", "failure_id", "id")?;
        let artifact =
            archive_optional_text_column(connection, &schema, "command_failures", "artifact_ref")?;
        let preview = archive_first_text_column(
            connection,
            &schema,
            "command_failures",
            &["response_preview", "cx_response"],
        )?;
        selects.push(format!(
            "
            SELECT '{schema}:' || {id} AS failure_id,
                   {artifact} AS artifact_ref,
                   {preview} AS response_preview
            FROM {schema}.command_failures
            "
        ));
    }
    Ok(selects)
}

fn archive_report_selects(connection: &Connection, paths: &[PathBuf]) -> Result<Vec<String>> {
    let mut selects = Vec::new();
    for index in 0..paths.len() {
        let schema = archive_schema(index);
        if !archive_table_exists(connection, &schema, "command_reports")? {
            continue;
        }
        let id = archive_text_column(connection, &schema, "command_reports", "report_id", "id")?;
        let preview = archive_first_text_column(
            connection,
            &schema,
            "command_reports",
            &["report_text", "note"],
        )?;
        selects.push(format!(
            "
            SELECT '{schema}:' || {id} AS report_id,
                   '' AS artifact_ref,
                   {preview} AS response_preview
            FROM {schema}.command_reports
            "
        ));
    }
    Ok(selects)
}

fn archive_command_total_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ArchiveCommandTotalInsight> {
    Ok(ArchiveCommandTotalInsight {
        command_family: row.get(0)?,
        process: row.get(1)?,
        invocations: row.get(2)?,
        nonzero: row.get(3)?,
        raw_tokens: row.get(4)?,
        emitted_tokens: row.get(5)?,
        saved_tokens: row.get(6)?,
    })
}

fn metrics_from_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<TextMetrics> {
    Ok(TextMetrics {
        bytes: row.get(offset)?,
        chars: row.get(offset + 1)?,
        lines: row.get(offset + 2)?,
        tokens: row.get(offset + 3)?,
    })
}

fn archive_table_exists(connection: &Connection, schema: &str, table: &str) -> Result<bool> {
    let sql =
        format!("SELECT COUNT(*) FROM {schema}.sqlite_master WHERE type = 'table' AND name = ?1");
    let exists: i64 = connection.query_row(&sql, [table], |row| row.get(0))?;
    Ok(exists > 0)
}

fn archive_column_exists(
    connection: &Connection,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA {schema}.table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for candidate in columns {
        if candidate? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn archive_text_column(
    connection: &Connection,
    schema: &str,
    table: &str,
    preferred: &str,
    fallback: &str,
) -> Result<String> {
    if archive_column_exists(connection, schema, table, preferred)? {
        return Ok(format!("CAST({preferred} AS TEXT)"));
    }
    if archive_column_exists(connection, schema, table, fallback)? {
        return Ok(format!("CAST({fallback} AS TEXT)"));
    }
    Ok("''".to_string())
}

fn archive_optional_text_column(
    connection: &Connection,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<String> {
    if archive_column_exists(connection, schema, table, column)? {
        return Ok(format!("COALESCE({column}, '')"));
    }
    Ok("''".to_string())
}

fn archive_command_shape_expression(
    connection: &Connection,
    schema: &str,
    table: &str,
) -> Result<String> {
    archive_shape_expression(
        connection,
        schema,
        table,
        "command_shape",
        &["$.commandShape", "$.command_shape"],
    )
}

fn archive_command_shape_hash_expression(
    connection: &Connection,
    schema: &str,
    table: &str,
) -> Result<String> {
    archive_shape_expression(
        connection,
        schema,
        table,
        "command_shape_hash",
        &["$.commandShapeHash", "$.command_shape_hash"],
    )
}

fn archive_shape_expression(
    connection: &Connection,
    schema: &str,
    table: &str,
    column: &str,
    metadata_paths: &[&str],
) -> Result<String> {
    let mut candidates = Vec::with_capacity(metadata_paths.len() + 1);
    if archive_column_exists(connection, schema, table, column)? {
        candidates.push(format!("NULLIF({column}, '')"));
    }
    for path in metadata_paths {
        candidates.push(format!("NULLIF(json_extract(metadata_json, '{path}'), '')"));
    }
    Ok(format!("COALESCE({}, '')", candidates.join(", ")))
}

fn archive_first_text_column(
    connection: &Connection,
    schema: &str,
    table: &str,
    columns: &[&str],
) -> Result<String> {
    for column in columns {
        if archive_column_exists(connection, schema, table, column)? {
            return Ok(format!("COALESCE({column}, '')"));
        }
    }
    Ok("''".to_string())
}

fn archive_schema(index: usize) -> String {
    if index == 0 {
        "main".to_string()
    } else {
        archive_alias(index)
    }
}

fn archive_alias(index: usize) -> String {
    format!("archive_{index}")
}

fn legacy_process_sql(column: &str) -> String {
    format!(
        "
        CASE
            WHEN {column} LIKE 'git %' THEN 'git'
            WHEN {column} LIKE 'cargo %' THEN 'cargo'
            WHEN {column} LIKE 'go %' THEN 'go'
            WHEN {column} LIKE 'cmake %' THEN 'cmake'
            WHEN {column} LIKE 'docker %' THEN 'docker'
            WHEN {column} LIKE 'kubectl %' THEN 'kubectl'
            WHEN {column} = 'node check' THEN 'node'
            WHEN {column} = 'grep files' THEN 'grep'
            WHEN {column} LIKE 'passthrough %' THEN 'passthrough'
            ELSE {column}
        END
        "
    )
}
