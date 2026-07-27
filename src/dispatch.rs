mod output;
mod read;
mod routing;
mod telemetry;

use anyhow::Result;

use crate::cli::{Cli, Command};

use output::emit_proxy_outcome;
use routing::execute_command;
use telemetry::record_insights;

pub fn execute(cli: &Cli) -> Result<i32> {
    let outcome = execute_command(&cli.command)?;
    let exit_code = emit_proxy_outcome(&outcome)?;
    if !matches!(
        cli.command,
        Command::Insights { .. } | Command::Report { .. }
    ) {
        record_insights(cli, &outcome);
    }
    Ok(exit_code)
}

#[cfg(test)]
mod tests;
