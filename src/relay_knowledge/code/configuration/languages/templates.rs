pub(super) fn strip_go_comments(value: &str, in_comment: &mut bool) -> String {
    let mut output = String::new();
    let mut rest = value;
    loop {
        if *in_comment {
            let Some(index) = go_comment_end(rest) else {
                return output;
            };
            *in_comment = false;
            rest = &rest[index..];
        }
        let Some((start, after_start)) = go_comment_start(rest) else {
            output.push_str(rest);
            return output;
        };
        output.push_str(&rest[..start]);
        rest = &rest[after_start..];
        *in_comment = true;
    }
}

pub(super) fn strip_jinja_comments(value: &str, in_comment: &mut bool) -> String {
    strip_block_comments(value, in_comment, "{#", "#}")
}

pub(super) fn quoted_values(value: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = value;
    while let Some(quote_index) = rest.find(['"', '\'']) {
        let quote = rest.as_bytes()[quote_index] as char;
        let value_start = quote_index + quote.len_utf8();
        let after_start = &rest[value_start..];
        let Some(value_end) = after_start.find(quote) else {
            break;
        };
        values.push(&after_start[..value_end]);
        rest = &after_start[value_end + quote.len_utf8()..];
    }

    values
}

fn strip_block_comments(value: &str, in_comment: &mut bool, start: &str, end: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    loop {
        if *in_comment {
            let Some(index) = rest.find(end) else {
                return output;
            };
            *in_comment = false;
            rest = &rest[index + end.len()..];
        }
        let Some(index) = rest.find(start) else {
            output.push_str(rest);
            return output;
        };
        output.push_str(&rest[..index]);
        rest = &rest[index + start.len()..];
        *in_comment = true;
    }
}

fn go_comment_start(value: &str) -> Option<(usize, usize)> {
    let mut offset = 0usize;
    while let Some(relative_index) = value[offset..].find("{{") {
        let start = offset + relative_index;
        let mut rest = &value[start + "{{".len()..];
        let mut after_start = start + "{{".len();
        let skipped = rest.len();
        rest = rest.trim_start();
        after_start += skipped - rest.len();
        if let Some(after_trim) = rest.strip_prefix('-') {
            after_start += '-'.len_utf8();
            rest = after_trim;
            let skipped = rest.len();
            rest = rest.trim_start();
            after_start += skipped - rest.len();
        }
        if rest.starts_with("/*") {
            return Some((start, after_start + "/*".len()));
        }
        offset = start + "{{".len();
    }

    None
}

fn go_comment_end(value: &str) -> Option<usize> {
    let index = value.find("*/")?;
    let mut rest = &value[index + "*/".len()..];
    let mut end = index + "*/".len();
    let skipped = rest.len();
    rest = rest.trim_start();
    end += skipped - rest.len();
    if let Some(after_trim) = rest.strip_prefix('-') {
        end += '-'.len_utf8();
        rest = after_trim;
        let skipped = rest.len();
        rest = rest.trim_start();
        end += skipped - rest.len();
    }
    if rest.starts_with("}}") {
        return Some(end + "}}".len());
    }

    Some(index + "*/".len())
}
