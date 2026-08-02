const MAX_GENERIC_CALL_LOOKAHEAD_BYTES: usize = 256;

pub(super) fn identifier_is_indirect_call(after: &str) -> bool {
    let Some(rest) = after.strip_prefix('[') else {
        return false;
    };
    let Some((_, tail)) = rest.split_once(']') else {
        return false;
    };
    tail.trim_start().starts_with('(')
}

pub(super) fn identifier_is_plain_call(after: &str) -> bool {
    after.starts_with('(') || identifier_is_generic_call(after)
}

fn identifier_is_generic_call(after: &str) -> bool {
    let Some(rest) = after.strip_prefix('<') else {
        return false;
    };
    let mut depth = 1usize;
    for (index, character) in rest.char_indices() {
        if index > MAX_GENERIC_CALL_LOOKAHEAD_BYTES {
            return false;
        }
        match character {
            '<' => depth = depth.saturating_add(1),
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let tail_start = index + character.len_utf8();
                    return rest
                        .get(tail_start..)
                        .unwrap_or_default()
                        .trim_start()
                        .starts_with('(');
                }
            }
            _ => {}
        }
    }

    false
}

pub(super) fn identifier_is_member_call(before: &str, after: &str) -> bool {
    after.starts_with('(')
        && (before.trim_end().ends_with('.') || before.trim_end().ends_with("->"))
}

#[cfg(test)]
#[path = "call_shape_tests.rs"]
mod tests;
