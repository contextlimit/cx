pub fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

pub fn numbered_lines(label: &str, count: usize) -> String {
    let mut output = String::new();
    for index in 0..count {
        output.push_str(&format!("{label} {index:04}\n"));
    }
    output
}
