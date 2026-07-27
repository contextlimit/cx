use anyhow::{Context, Result};

use crate::cli::ReadCliMode;
use crate::commands;

#[derive(Debug, Clone, Copy)]
pub(super) struct ReadDispatchArgs<'a> {
    pub(super) head: Option<usize>,
    pub(super) tail: Option<usize>,
    pub(super) range_spec: Option<&'a str>,
    pub(super) full: bool,
    pub(super) line_numbers: bool,
    pub(super) raw: bool,
    pub(super) mode: Option<ReadCliMode>,
    pub(super) smart: bool,
    pub(super) max_lines: Option<usize>,
    pub(super) no_auto_aggressive: bool,
}

pub(super) fn build_read_options(
    args: ReadDispatchArgs<'_>,
) -> Result<commands::read::ReadOptions> {
    let mode = selected_read_mode(args.mode, args.smart);
    let line_range = parse_read_range(args.range_spec)?;
    if matches!(mode, commands::read::ReadMode::Smart)
        && (args.head.is_some()
            || args.tail.is_some()
            || line_range.is_some()
            || args.full
            || args.line_numbers
            || args.raw
            || args.max_lines.is_some())
    {
        anyhow::bail!(
            "`read --mode smart` conflicts with `--head`, `--tail`, `--range`, `--line-numbers`, `--raw`, `--full`, and `--max-lines`"
        );
    }
    if line_range.is_some()
        && (!matches!(mode, commands::read::ReadMode::Normal) || args.max_lines.is_some())
    {
        anyhow::bail!("`read --range` conflicts with `--mode` and `--max-lines`");
    }
    Ok(commands::read::ReadOptions {
        head_lines: if args.full { None } else { args.head },
        tail_lines: if args.full { None } else { args.tail },
        line_range,
        line_numbers: args.line_numbers,
        raw: args.raw,
        mode,
        max_lines: args.max_lines,
        auto_aggressive: !args.no_auto_aggressive && !args.full,
    })
}

pub(super) fn parse_read_range(
    range_spec: Option<&str>,
) -> Result<Option<commands::read::ReadRange>> {
    let Some(range_spec) = range_spec else {
        return Ok(None);
    };
    let trimmed = range_spec.trim();
    if trimmed.is_empty() {
        anyhow::bail!("`read --range` requires a non-empty value");
    }

    if let Some((start, end)) = trimmed.split_once(':') {
        let start = parse_optional_line_bound(start, "range start")?;
        let end = parse_optional_line_bound(end, "range end")?;
        if start.is_none() && end.is_none() {
            anyhow::bail!("`read --range` must include at least one line bound");
        }
        if let (Some(start), Some(end)) = (start, end) {
            if end < start {
                anyhow::bail!("`read --range` end must be greater than or equal to start");
            }
        }
        return Ok(Some(commands::read::ReadRange { start, end }));
    }

    let line = parse_required_line_bound(trimmed, "range")?;
    Ok(Some(commands::read::ReadRange {
        start: Some(line),
        end: Some(line),
    }))
}

fn parse_optional_line_bound(value: &str, label: &str) -> Result<Option<usize>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(parse_required_line_bound(trimmed, label)?))
}

fn parse_required_line_bound(value: &str, label: &str) -> Result<usize> {
    let parsed = value
        .parse::<usize>()
        .with_context(|| format!("invalid {label} `{value}`"))?;
    if parsed == 0 {
        anyhow::bail!("`read --range` line numbers start at 1");
    }
    Ok(parsed)
}

fn selected_read_mode(mode: Option<ReadCliMode>, smart: bool) -> commands::read::ReadMode {
    match (mode, smart) {
        (_, true) | (Some(ReadCliMode::Smart), false) => commands::read::ReadMode::Smart,
        (Some(ReadCliMode::Aggressive), false) => commands::read::ReadMode::Aggressive,
        _ => commands::read::ReadMode::Normal,
    }
}
