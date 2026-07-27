use std::collections::BTreeMap;

use anyhow::Result;

use crate::support::{
    failure_artifact::{self, FailureArtifactSummary},
    insights::{
        self, CommandFilter, CommandLevel, CommandTotalInsight, CommandTotalSort,
        FailureCoverageInsight,
    },
};

use super::artifacts::failure_artifact_tool_name;

#[derive(Debug, Clone)]
pub(crate) struct FailureFocus {
    pub(crate) total: CommandTotalInsight,
    pub(crate) coverage: FailureCoverageInsight,
    pub(crate) artifact_summary: FailureArtifactSummary,
}

impl FailureFocus {
    pub(crate) fn has_output_gap(&self) -> bool {
        self.coverage.output_gap_detail_rows > 0
    }

    pub(crate) fn coverage_unknown(&self) -> bool {
        self.coverage.unknown_invocations > 0
    }

    pub(crate) fn linked_but_pruned(&self) -> bool {
        self.coverage.artifact_linked_detail_rows > 0 && self.artifact_summary.count == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FailureCoverageSummary {
    pub(crate) failing_groups: u64,
    pub(crate) failed_invocations: u64,
    pub(crate) detail_rows: u64,
    pub(crate) linked_detail_rows: u64,
    pub(crate) orphan_detail_rows: u64,
    pub(crate) unknown_invocations: u64,
    pub(crate) output_bearing_detail_rows: u64,
    pub(crate) silent_detail_rows: u64,
    pub(crate) artifact_linked_detail_rows: u64,
    pub(crate) output_gap_detail_rows: u64,
    pub(crate) groups_with_output_gaps: u64,
    pub(crate) groups_with_unknown_coverage: u64,
    pub(crate) groups_with_linked_but_pruned_artifacts: u64,
    pub(crate) groups_with_retained_artifacts: u64,
}

impl FailureCoverageSummary {
    fn from_rows(rows: &[FailureFocus]) -> Self {
        let mut summary = Self::default();
        for row in rows {
            let coverage = &row.coverage;
            summary.failing_groups += u64::from(coverage.failed_invocations > 0);
            summary.failed_invocations += coverage.failed_invocations;
            summary.detail_rows += coverage.detail_rows;
            summary.linked_detail_rows += coverage.linked_detail_rows;
            summary.orphan_detail_rows += coverage.orphan_detail_rows;
            summary.unknown_invocations += coverage.unknown_invocations;
            summary.output_bearing_detail_rows += coverage.output_bearing_detail_rows;
            summary.silent_detail_rows += coverage.silent_detail_rows;
            summary.artifact_linked_detail_rows += coverage.artifact_linked_detail_rows;
            summary.output_gap_detail_rows += coverage.output_gap_detail_rows;
            summary.groups_with_output_gaps += u64::from(row.has_output_gap());
            summary.groups_with_unknown_coverage += u64::from(row.coverage_unknown());
            summary.groups_with_linked_but_pruned_artifacts += u64::from(row.linked_but_pruned());
            summary.groups_with_retained_artifacts += u64::from(row.artifact_summary.count > 0);
        }
        summary
    }
}

pub(crate) struct FailureCoverageEvidence {
    pub(crate) summary: FailureCoverageSummary,
    pub(crate) rows: Vec<FailureFocus>,
}

pub(crate) fn load_failure_coverage(
    level: CommandLevel,
    filter: CommandFilter<'_>,
) -> Result<FailureCoverageEvidence> {
    let totals = insights::command_totals_at_level_filtered(
        level,
        CommandTotalSort::Failures,
        1_000,
        filter,
    )?;
    let coverage = insights::failure_coverage_at_level_filtered(level, filter)?;
    let mut totals_by_command = totals
        .into_iter()
        .map(|total| (total.command.clone(), total))
        .collect::<BTreeMap<_, _>>();
    let mut coverage_by_command = coverage
        .into_iter()
        .map(|item| (item.command.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let commands = totals_by_command
        .keys()
        .chain(coverage_by_command.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut artifact_summaries = BTreeMap::<String, FailureArtifactSummary>::new();
    let mut rows = Vec::new();
    for command in commands {
        let mut total = totals_by_command.remove(&command).unwrap_or_default();
        total.command = command.clone();
        let mut coverage = coverage_by_command.remove(&command).unwrap_or_default();
        coverage.command = command.clone();
        let tool = failure_artifact_tool_name(&command, level);
        let artifact_summary = if let Some(summary) = artifact_summaries.get(&tool) {
            summary.clone()
        } else {
            let summary = failure_artifact::failure_artifact_summary(&tool)?;
            artifact_summaries.insert(tool, summary.clone());
            summary
        };
        rows.push(FailureFocus {
            total,
            coverage,
            artifact_summary,
        });
    }
    rows.retain(|row| row.coverage.failed_invocations > 0 || row.coverage.detail_rows > 0);
    rows.sort_by(|left, right| {
        right
            .coverage
            .output_gap_detail_rows
            .cmp(&left.coverage.output_gap_detail_rows)
            .then_with(|| {
                right
                    .coverage
                    .unknown_invocations
                    .cmp(&left.coverage.unknown_invocations)
            })
            .then_with(|| {
                right
                    .coverage
                    .failed_invocations
                    .cmp(&left.coverage.failed_invocations)
            })
            .then_with(|| left.total.command.cmp(&right.total.command))
    });
    Ok(FailureCoverageEvidence {
        summary: FailureCoverageSummary::from_rows(&rows),
        rows,
    })
}
