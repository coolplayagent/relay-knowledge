use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

pub(super) fn exact_caller_named_receiver_member_call_bonus(
    base_score: f64,
    query: &str,
    caller_name: Option<&str>,
    caller_excerpt: Option<&str>,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Callees
        || !query_leaf_matches_caller(query, caller_name)
    {
        return 0.0;
    }
    let Some(caller_excerpt) = caller_excerpt else {
        return 0.0;
    };
    if callee_name.trim().is_empty() {
        return 0.0;
    }

    if caller_excerpt
        .lines()
        .any(|line| line_contains_named_receiver_member_call_to(line, callee_name))
    {
        5.0
    } else {
        0.0
    }
}

fn line_contains_named_receiver_member_call_to(line: &str, callee_name: &str) -> bool {
    let mut search_start = 0;
    while let Some(relative_index) = line[search_start..].find(callee_name) {
        let start = search_start + relative_index;
        let end = start + callee_name.len();
        if named_receiver_member_call_prefix(&line[..start]) && call_suffix(&line[end..]) {
            return true;
        }
        search_start = end;
    }

    false
}

fn named_receiver_member_call_prefix(prefix: &str) -> bool {
    let prefix = prefix.trim_end();
    let Some(receiver) = prefix
        .strip_suffix('.')
        .or_else(|| prefix.strip_suffix("::"))
        .or_else(|| prefix.strip_suffix("->"))
    else {
        return false;
    };
    receiver_leaf_is_type_like(receiver)
}

fn receiver_leaf_is_type_like(receiver: &str) -> bool {
    let Some(leaf) = receiver
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|term| !term.is_empty())
    else {
        return false;
    };
    leaf.chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
        || leaf_has_case_boundary(leaf)
}

fn leaf_has_case_boundary(leaf: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in leaf.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            return true;
        }
        previous = Some(character);
    }

    false
}

fn call_suffix(suffix: &str) -> bool {
    let suffix = suffix.trim_start();
    suffix.starts_with('(') || (suffix.starts_with('<') && suffix.contains('('))
}

fn query_leaf_matches_caller(query: &str, caller_name: Option<&str>) -> bool {
    let Some(caller_name) = caller_name.map(str::trim).filter(|name| !name.is_empty()) else {
        return false;
    };
    query
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|term| !term.is_empty())
        .is_some_and(|leaf| leaf.eq_ignore_ascii_case(caller_name))
}
