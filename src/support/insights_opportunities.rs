use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use super::{
    bounded_limit, create_schema, insights_db_file, now_ms, open_readonly_database, ratio,
    settings, table_exists, SavedMetrics, TextMetrics,
};

#[derive(Debug, Clone)]
pub struct CommandOpportunityRecord<'a> {
    pub process: &'a str,
    pub command_family: &'a str,
    pub command: &'a str,
    pub source: &'a str,
    pub strategy: &'a str,
    pub confidence: OpportunityConfidence,
    pub raw: TextMetrics,
    pub projected: TextMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOpportunityInsight {
    pub process: String,
    pub command_family: String,
    pub samples: u64,
    pub latest_at_ms: u64,
    pub raw: TextMetrics,
    pub projected: TextMetrics,
    pub potential_saved: TextMetrics,
    pub best_potential_saved_tokens: u64,
    pub strategy: String,
    pub confidence: OpportunityConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpportunityConfidence {
    High,
    Medium,
    #[default]
    Low,
}

impl OpportunityConfidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::High => 3,
            Self::Medium => 2,
            Self::Low => 1,
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpportunityFilter {
    pub since_ms: Option<u64>,
    pub minimum_confidence: OpportunityConfidence,
}

pub fn record_command_opportunity(record: &CommandOpportunityRecord<'_>) -> Result<bool> {
    let settings = settings::recording_settings()?;
    if !settings.record_invocations {
        return Ok(false);
    }
    let saved = SavedMetrics::from_raw_and_emitted(record.raw, record.projected);
    if saved.bytes == 0 && saved.chars == 0 && saved.lines == 0 && saved.tokens == 0 {
        return Ok(false);
    }

    let db_path = insights_db_file()?;
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let connection = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection.busy_timeout(Duration::from_millis(100))?;
    create_schema(&connection)?;
    insert_command_opportunity(&connection, record, saved, settings)?;
    Ok(true)
}

pub fn command_opportunities(limit: usize) -> Result<Vec<CommandOpportunityInsight>> {
    command_opportunities_filtered(limit, OpportunityFilter::default())
}

pub fn command_opportunities_filtered(
    limit: usize,
    filter: OpportunityFilter,
) -> Result<Vec<CommandOpportunityInsight>> {
    let Some(connection) = open_readonly_database()? else {
        return Ok(Vec::new());
    };
    if !table_exists(&connection, "command_opportunities")? {
        return Ok(Vec::new());
    }
    let confidence = if super::column_exists(&connection, "command_opportunities", "confidence")? {
        "confidence"
    } else {
        "'low'"
    };

    let mut statement = connection.prepare(&format!(
        "
        SELECT process, command_family, COUNT(*) AS samples, MAX(occurred_at_ms) AS latest_at_ms,
               SUM(raw_bytes), SUM(raw_chars), SUM(raw_lines), SUM(raw_tokens),
               SUM(projected_bytes), SUM(projected_chars), SUM(projected_lines), SUM(projected_tokens),
               SUM(potential_saved_bytes), SUM(potential_saved_chars),
               SUM(potential_saved_lines), SUM(potential_saved_tokens),
               MAX(potential_saved_tokens), strategy, {confidence}
        FROM command_opportunities
        WHERE (?1 IS NULL OR occurred_at_ms >= ?1)
          AND CASE {confidence} WHEN 'high' THEN 3 WHEN 'medium' THEN 2 ELSE 1 END >= ?2
        GROUP BY process, command_family, strategy, {confidence}
        ORDER BY SUM(potential_saved_tokens) DESC, SUM(potential_saved_chars) DESC, samples DESC
        LIMIT ?3
        ",
    ))?;
    let rows = statement.query_map(
        params![
            filter.since_ms,
            filter.minimum_confidence.rank(),
            bounded_limit(limit)
        ],
        |row| {
            Ok(CommandOpportunityInsight {
                process: row.get(0)?,
                command_family: row.get(1)?,
                samples: row.get(2)?,
                latest_at_ms: row.get(3)?,
                raw: TextMetrics {
                    bytes: row.get(4)?,
                    chars: row.get(5)?,
                    lines: row.get(6)?,
                    tokens: row.get(7)?,
                },
                projected: TextMetrics {
                    bytes: row.get(8)?,
                    chars: row.get(9)?,
                    lines: row.get(10)?,
                    tokens: row.get(11)?,
                },
                potential_saved: TextMetrics {
                    bytes: row.get(12)?,
                    chars: row.get(13)?,
                    lines: row.get(14)?,
                    tokens: row.get(15)?,
                },
                best_potential_saved_tokens: row.get(16)?,
                strategy: row.get(17)?,
                confidence: OpportunityConfidence::parse(&row.get::<_, String>(18)?),
            })
        },
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub(super) fn create_command_opportunities_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS command_opportunities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at_ms INTEGER NOT NULL,
            process TEXT NOT NULL DEFAULT '',
            command_family TEXT NOT NULL DEFAULT '',
            command TEXT NOT NULL,
            source TEXT NOT NULL,
            strategy TEXT NOT NULL,
            confidence TEXT NOT NULL DEFAULT 'low' CHECK (
                confidence IN ('high', 'medium', 'low')
            ),
            raw_bytes INTEGER NOT NULL,
            raw_chars INTEGER NOT NULL,
            raw_lines INTEGER NOT NULL,
            raw_tokens INTEGER NOT NULL,
            projected_bytes INTEGER NOT NULL,
            projected_chars INTEGER NOT NULL,
            projected_lines INTEGER NOT NULL,
            projected_tokens INTEGER NOT NULL,
            potential_saved_bytes INTEGER NOT NULL,
            potential_saved_chars INTEGER NOT NULL,
            potential_saved_lines INTEGER NOT NULL,
            potential_saved_tokens INTEGER NOT NULL,
            potential_savings_ratio REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_command_opportunities_family_time
            ON command_opportunities(command_family, occurred_at_ms);
        CREATE INDEX IF NOT EXISTS idx_command_opportunities_process_time
            ON command_opportunities(process, occurred_at_ms);
        ",
    )?;
    if !super::column_exists(connection, "command_opportunities", "confidence")? {
        connection.execute(
            "ALTER TABLE command_opportunities ADD COLUMN confidence TEXT NOT NULL DEFAULT 'low'",
            [],
        )?;
    }
    Ok(())
}

fn insert_command_opportunity(
    connection: &Connection,
    record: &CommandOpportunityRecord<'_>,
    saved: SavedMetrics,
    settings: settings::InsightSettings,
) -> Result<()> {
    let command = if settings.record_command_text {
        record.command
    } else {
        record.command_family
    };
    let source = if settings.record_sources {
        record.source
    } else {
        "not recorded"
    };
    connection.execute(
        "
        INSERT INTO command_opportunities (
            occurred_at_ms, process, command_family, command, source, strategy, confidence,
            raw_bytes, raw_chars, raw_lines, raw_tokens,
            projected_bytes, projected_chars, projected_lines, projected_tokens,
            potential_saved_bytes, potential_saved_chars,
            potential_saved_lines, potential_saved_tokens,
            potential_savings_ratio
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
        ",
        params![
            now_ms(),
            record.process,
            record.command_family,
            command,
            source,
            record.strategy,
            record.confidence.as_str(),
            record.raw.bytes,
            record.raw.chars,
            record.raw.lines,
            record.raw.tokens,
            record.projected.bytes,
            record.projected.chars,
            record.projected.lines,
            record.projected.tokens,
            saved.bytes,
            saved.chars,
            saved.lines,
            saved.tokens,
            ratio(saved.chars, record.raw.chars),
        ],
    )?;
    Ok(())
}
