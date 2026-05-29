pub(super) fn skip_continued_value_line(line: &str, trimmed: &str, active: &mut bool) -> bool {
    if *active {
        *active = value_continues(line);
        return true;
    }
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return false;
    }
    *active = value_continues(line);
    false
}

fn value_continues(line: &str) -> bool {
    let trailing = line.trim_end();
    let backslashes = trailing
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count();
    backslashes % 2 == 1
}
