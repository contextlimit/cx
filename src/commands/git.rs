mod diff;
mod evidence;
mod history;
mod status;

pub use diff::run_diff;
pub use evidence::{run_conflict_diff, run_evidence_diff};
pub use history::{run_log, run_show};
pub use status::run_status;

#[cfg(test)]
mod tests;
