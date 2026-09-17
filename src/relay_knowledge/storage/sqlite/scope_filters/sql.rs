//! Shared bounded SQL path predicates for sibling projection owners.
use rusqlite::types::Value;

pub(in crate::storage::sqlite) fn path_filter_sql_for_column(
    column: &str,
    filters: &[String],
) -> String {
    let clauses = filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
        .map(|_| format!("({column} = ? OR {column} LIKE ? ESCAPE '\\')"))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

pub(in crate::storage::sqlite) fn push_path_filter_values(
    values: &mut Vec<Value>,
    filters: &[String],
) {
    for filter in filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
    {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_sql_like(&filter))));
    }
}

fn normalized_sql_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    (!filter.is_empty() && filter != ".").then(|| filter.to_owned())
}

fn escape_sql_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
