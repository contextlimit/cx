use anyhow::Result;
use rusqlite::Connection;

pub(super) fn argv_json_query_expression(connection: &Connection) -> Result<String> {
    if super::column_exists(connection, "command_invocations", "argv_json")? {
        Ok("argv_json".to_string())
    } else {
        Ok("'[]'".to_string())
    }
}

pub(super) fn process_query_expression(
    connection: &Connection,
    alias: Option<&str>,
) -> Result<String> {
    if super::column_exists(connection, "command_invocations", "process")? {
        return Ok(format!(
            "COALESCE(NULLIF({}, ''), {})",
            qualified_column(alias, "process"),
            legacy_process_sql(&qualified_column(alias, "command"))
        ));
    }
    if super::column_exists(connection, "command_invocations", "command_root")? {
        return Ok(format!(
            "COALESCE(NULLIF({}, ''), {})",
            qualified_column(alias, "command_root"),
            legacy_process_sql(&qualified_column(alias, "command"))
        ));
    }
    Ok(legacy_process_sql(&qualified_column(alias, "command")))
}

pub(super) fn command_family_query_expression(
    connection: &Connection,
    alias: Option<&str>,
) -> Result<String> {
    if super::column_exists(connection, "command_invocations", "command_family")? {
        Ok(format!(
            "COALESCE(NULLIF({}, ''), {})",
            qualified_column(alias, "command_family"),
            qualified_column(alias, "command")
        ))
    } else {
        Ok(qualified_column(alias, "command"))
    }
}

pub(super) fn source_query_expression(
    connection: &Connection,
    alias: Option<&str>,
) -> Result<String> {
    if super::column_exists(connection, "command_invocations", "source")? {
        return Ok(qualified_column(alias, "source"));
    }
    if super::column_exists(connection, "command_invocations", "raw_source")? {
        return Ok(qualified_column(alias, "raw_source"));
    }
    Ok("''".to_string())
}

pub(super) fn optional_text_column_query_expression(
    connection: &Connection,
    alias: Option<&str>,
    column: &str,
) -> Result<String> {
    optional_text_column_query_expression_for_table(
        connection,
        "command_invocations",
        alias,
        column,
    )
}

pub(super) fn optional_text_column_query_expression_for_table(
    connection: &Connection,
    table: &str,
    alias: Option<&str>,
    column: &str,
) -> Result<String> {
    if super::column_exists(connection, table, column)? {
        return Ok(qualified_column(alias, column));
    }
    Ok("''".to_string())
}

pub(super) fn optional_integer_column_query_expression(
    connection: &Connection,
    alias: Option<&str>,
    column: &str,
) -> Result<String> {
    optional_integer_column_query_expression_for_table(
        connection,
        "command_invocations",
        alias,
        column,
    )
}

pub(super) fn optional_integer_column_query_expression_for_table(
    connection: &Connection,
    table: &str,
    alias: Option<&str>,
    column: &str,
) -> Result<String> {
    if super::column_exists(connection, table, column)? {
        return Ok(qualified_column(alias, column));
    }
    Ok("0".to_string())
}

pub(super) fn nullable_integer_column_query_expression_for_table(
    connection: &Connection,
    table: &str,
    alias: Option<&str>,
    column: &str,
) -> Result<String> {
    if super::column_exists(connection, table, column)? {
        return Ok(qualified_column(alias, column));
    }
    Ok("NULL".to_string())
}

pub(super) fn classified_failure_sql(
    connection: &Connection,
    alias: Option<&str>,
) -> Result<String> {
    let command_family = command_family_query_expression(connection, alias)?;
    let exit_code = qualified_column(alias, "exit_code");
    Ok(format!(
        "
        CASE
            WHEN {command_family} = 'grep'
              OR {command_family} LIKE 'grep %'
              OR {command_family} = 'rg'
              OR {command_family} LIKE 'rg %'
            THEN CASE WHEN {exit_code} > 1 THEN 1 ELSE 0 END
            WHEN {exit_code} != 0 THEN 1
            ELSE 0
        END
        "
    ))
}

pub(super) fn command_report_root_query_expression(connection: &Connection) -> Result<String> {
    if super::column_exists(connection, "command_reports", "command_root")? {
        Ok(format!(
            "COALESCE(NULLIF(command_root, ''), {})",
            legacy_process_sql("command_family")
        ))
    } else {
        Ok(legacy_process_sql("command_family"))
    }
}

pub(super) fn legacy_process_sql(column: &str) -> String {
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
            ELSE {column}
        END
        "
    )
}

fn qualified_column(alias: Option<&str>, column: &str) -> String {
    if let Some(alias) = alias {
        format!("{alias}.{column}")
    } else {
        column.to_string()
    }
}
