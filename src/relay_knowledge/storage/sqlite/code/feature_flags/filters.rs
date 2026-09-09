use rusqlite::types::Value;

/// Share Unicode word boundaries between SQL seeds and assembled-group matching.
pub(super) fn query_terms(query: &str) -> impl Iterator<Item = &str> {
    query
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
}

pub(super) fn append_path_filter_clause(
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
    filters: &[String],
) {
    if filters.is_empty() {
        return;
    }

    let mut fragments = Vec::new();
    for filter in filters {
        let filter = normalize_sql_path_filter(filter);
        if filter == "." {
            return;
        }
        if filter.is_empty() {
            continue;
        }
        fragments.push("(flag.path = ? OR flag.path LIKE ? ESCAPE '\\')".to_owned());
        params.push(Value::Text(filter.to_owned()));
        params.push(Value::Text(format!("{}/%", escape_like_pattern(filter))));
    }

    if fragments.is_empty() {
        clauses.push("0 = 1".to_owned());
    } else {
        clauses.push(format!("({})", fragments.join(" OR ")));
    }
}

pub(super) fn append_language_filter_clause(
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
    filters: &[String],
) {
    if filters.is_empty() {
        return;
    }

    let mut unique = Vec::<&str>::new();
    for filter in filters {
        if !filter.is_empty() && !unique.contains(&filter.as_str()) {
            unique.push(filter);
        }
    }
    if unique.is_empty() {
        clauses.push("0 = 1".to_owned());
        return;
    }

    clauses.push(format!(
        "flag.language_id IN ({})",
        vec!["?"; unique.len()].join(", ")
    ));
    for filter in unique {
        params.push(Value::Text(filter.to_owned()));
    }
}

fn normalize_sql_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

fn escape_like_pattern(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }

    escaped
}

#[cfg(test)]
#[path = "filters_tests.rs"]
mod tests;
