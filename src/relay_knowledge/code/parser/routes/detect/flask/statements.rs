const MAX_FLASK_ROUTE_DECORATOR_LINES: usize = 12;
const MAX_PYTHON_ROUTER_PREFIX_LINES: usize = 12;

pub(super) fn python_router_prefix_statement(
    lines: &[String],
    start: usize,
) -> Option<(String, usize)> {
    let first_line = lines[start].trim();
    if !first_line.contains('=')
        || (!first_line.contains("APIRouter(")
            && !first_line.contains("FastAPI(")
            && !first_line.contains("Blueprint("))
    {
        return None;
    }
    Some(python_parenthesized_statement(
        lines,
        start,
        MAX_PYTHON_ROUTER_PREFIX_LINES,
    ))
}

pub(super) fn python_include_router_statement(
    lines: &[String],
    start: usize,
) -> Option<(String, usize)> {
    let first_line = lines[start].trim();
    if !first_line.contains(".include_router(") {
        return None;
    }
    Some(python_parenthesized_statement(
        lines,
        start,
        MAX_PYTHON_ROUTER_PREFIX_LINES,
    ))
}

pub(super) fn python_register_blueprint_statement(
    lines: &[String],
    start: usize,
) -> Option<(String, usize)> {
    let first_line = lines[start].trim();
    if !first_line.contains(".register_blueprint(") {
        return None;
    }
    Some(python_parenthesized_statement(
        lines,
        start,
        MAX_PYTHON_ROUTER_PREFIX_LINES,
    ))
}

pub(super) fn python_add_url_rule_statement(
    lines: &[String],
    start: usize,
) -> Option<(String, usize)> {
    let first_line = lines[start].trim();
    if !first_line.contains(".add_url_rule(") {
        return None;
    }
    Some(python_parenthesized_statement(
        lines,
        start,
        MAX_FLASK_ROUTE_DECORATOR_LINES,
    ))
}

pub(super) fn flask_decorator_statement(lines: &[String], start: usize) -> (String, usize) {
    python_parenthesized_statement(lines, start, MAX_FLASK_ROUTE_DECORATOR_LINES)
}

fn python_parenthesized_statement(
    lines: &[String],
    start: usize,
    max_lines: usize,
) -> (String, usize) {
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut saw_open = false;
    let mut consumed = 0usize;
    for line in lines.iter().skip(start).take(max_lines) {
        let segment = line.trim();
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        consumed += 1;
        for character in segment.chars() {
            if let Some(quote_char) = quote {
                if escaped {
                    escaped = false;
                    continue;
                }
                if character == '\\' {
                    escaped = true;
                    continue;
                }
                if character == quote_char {
                    quote = None;
                }
                continue;
            }
            match character {
                '\'' | '"' => quote = Some(character),
                '(' => {
                    depth += 1;
                    saw_open = true;
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    if saw_open && depth == 0 {
                        return (statement, consumed);
                    }
                }
                _ => {}
            }
        }
        if !saw_open {
            return (statement, consumed);
        }
    }
    (statement, consumed.max(1))
}

#[cfg(test)]
#[path = "statements_tests.rs"]
mod tests;
