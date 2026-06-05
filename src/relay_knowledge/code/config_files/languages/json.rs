pub(in crate::code::config_files) fn object_keys(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut keys = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }

        let start = index + 1;
        index += 1;
        let mut escaped = false;
        while index < bytes.len() {
            if escaped {
                escaped = false;
            } else if bytes[index] == b'\\' {
                escaped = true;
            } else if bytes[index] == b'"' {
                break;
            }
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }

        let end = index;
        let mut after = index + 1;
        while after < bytes.len() && bytes[after].is_ascii_whitespace() {
            after += 1;
        }
        if bytes.get(after) == Some(&b':') {
            keys.push(&line[start..end]);
        }
        index += 1;
    }

    keys
}
