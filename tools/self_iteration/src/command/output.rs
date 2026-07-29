pub fn last_output_line(stdout: &str, stderr: &str) -> String {
    for output in [stderr, stdout] {
        if let Some(line) = output.lines().map(str::trim).rfind(|line| !line.is_empty()) {
            return tail(line, 400);
        }
    }
    String::new()
}

pub fn tail(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_owned();
    }
    value.chars().skip(count - max_chars).collect()
}
