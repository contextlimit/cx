pub(super) fn format_ratio(part: u64, whole: u64) -> String {
    format!("{:.1}%", ratio_value(part, whole) * 100.0)
}

pub(super) fn ratio_value(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

pub(crate) fn signed_delta(after: u64, before: u64) -> i64 {
    let delta = i128::from(after) - i128::from(before);
    delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

pub(super) fn format_signed_count(value: i64) -> String {
    if value < 0 {
        format!("-{}", format_count(value.unsigned_abs()))
    } else {
        format!("+{}", format_count(value as u64))
    }
}

pub(super) fn format_count(value: u64) -> String {
    let text = value.to_string();
    let mut output = String::with_capacity(text.len() + text.len() / 3);
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    output.chars().rev().collect()
}

pub(super) fn div_floor(value: u64, divisor: u64) -> u64 {
    value.checked_div(divisor).unwrap_or(0)
}
