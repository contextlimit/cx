use std::collections::BTreeSet;

pub(crate) struct FailureSelection {
    pub(crate) selected: Vec<bool>,
    pub(crate) repeated_warning_count: usize,
    pub(crate) omitted_unique_warnings: usize,
}

pub(crate) fn select(lines: &[&str]) -> Option<FailureSelection> {
    let failure_indices = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| is_failure_anchor(line).then_some(index))
        .collect::<Vec<_>>();
    let mut seen_warnings = BTreeSet::new();
    let mut unique_warning_indices = Vec::new();
    let mut repeated_warning_count = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if !is_warning(line) {
            continue;
        }
        if seen_warnings.insert(line.trim()) {
            unique_warning_indices.push(index);
        } else {
            repeated_warning_count += 1;
        }
    }
    let other_diagnostic_indices = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            (is_diagnostic(line) && !is_failure_anchor(line) && !is_warning(line)).then_some(index)
        })
        .collect::<Vec<_>>();
    if failure_indices.is_empty()
        && unique_warning_indices.is_empty()
        && other_diagnostic_indices.is_empty()
    {
        return None;
    }

    let mut selected = vec![false; lines.len()];
    let selected_failure_indices = edge_indices(&failure_indices, 24, 12);
    mark_indices(&mut selected, &selected_failure_indices);
    mark_terminal(&mut selected, 16);

    let context = context_indices(&selected_failure_indices, lines.len(), 1, 3);
    mark_edges(&mut selected, &context, 28, 12);
    mark_edges(&mut selected, &unique_warning_indices, 12, 4);
    mark_edges(&mut selected, &other_diagnostic_indices, 6, 2);
    deduplicate_selected_warnings(lines, &mut selected);

    let selected_warning_keys = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (selected[index] && is_warning(line)).then_some(line.trim()))
        .collect::<BTreeSet<_>>();
    let omitted_unique_warnings = seen_warnings
        .iter()
        .filter(|warning| !selected_warning_keys.contains(**warning))
        .count();
    Some(FailureSelection {
        selected,
        repeated_warning_count,
        omitted_unique_warnings,
    })
}

pub(crate) fn is_diagnostic(line: &str) -> bool {
    let trimmed = line.trim();
    is_failure_anchor(trimmed)
        || is_warning(trimmed)
        || trimmed.starts_with("ld:")
        || trimmed.starts_with("clang:")
        || trimmed.starts_with("clang++:")
        || trimmed.starts_with("cc:")
        || trimmed.starts_with("c++:")
}

fn is_failure_anchor(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("error:")
        || lower.contains(": error ")
        || lower.contains("fatal error")
        || lower.contains("failed")
        || lower.contains("undefined reference")
        || lower.contains("duplicate symbol")
        || lower.contains("no such file")
        || lower.contains("cannot find")
        || lower.contains("could not find")
        || lower.contains("linker command failed")
        || lower.contains("cmake error")
        || lower.contains("ninja: build stopped")
        || lower.contains("the command could not be loaded")
        || lower.contains("sdk was not found")
        || lower.starts_with("requested sdk version:")
        || trimmed.starts_with("FAILED:")
        || is_make_failure_line(&lower)
}

fn is_warning(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower.contains("warning:")
        || lower.contains(": warning ")
        || lower.starts_with("warning ")
        || lower.starts_with("cmake warning")
}

fn is_make_failure_line(lower: &str) -> bool {
    (lower.starts_with("make") || lower.starts_with("gmake") || lower.starts_with("nmake"))
        && lower.contains("***")
        && lower.contains("error")
}

fn edge_indices(indices: &[usize], head: usize, tail: usize) -> Vec<usize> {
    let mut selected = indices
        .iter()
        .take(head)
        .chain(indices.iter().rev().take(tail))
        .copied()
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    selected
}

fn context_indices(
    indices: &[usize],
    line_count: usize,
    before: usize,
    after: usize,
) -> Vec<usize> {
    let mut context = BTreeSet::new();
    for index in indices {
        let start = index.saturating_sub(before);
        let end = (index + after + 1).min(line_count);
        for context_index in start..end {
            if context_index != *index {
                context.insert(context_index);
            }
        }
    }
    context.into_iter().collect()
}

fn mark_indices(selected: &mut [bool], indices: &[usize]) {
    for index in indices {
        selected[*index] = true;
    }
}

fn mark_terminal(selected: &mut [bool], count: usize) {
    let start = selected.len().saturating_sub(count);
    for keep in &mut selected[start..] {
        *keep = true;
    }
}

fn mark_edges(selected: &mut [bool], indices: &[usize], head: usize, tail: usize) {
    for index in indices.iter().take(head) {
        selected[*index] = true;
    }
    for index in indices.iter().rev().take(tail) {
        selected[*index] = true;
    }
}

fn deduplicate_selected_warnings(lines: &[&str], selected: &mut [bool]) {
    let mut seen = BTreeSet::new();
    for (index, line) in lines.iter().enumerate() {
        if selected[index] && is_warning(line) && !seen.insert(line.trim()) {
            selected[index] = false;
        }
    }
}
