use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::code_query_excerpts::line_declares_local_callable;

pub(super) const LOCAL_CALLABLE_DECLARATION_BONUS: f64 = 1.8;

pub(super) fn local_callable_declaration_bonus(
    base_score: f64,
    caller_excerpt: Option<&str>,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::Callees {
        return 0.0;
    }
    let Some(caller_excerpt) = caller_excerpt else {
        return 0.0;
    };
    if caller_excerpt.lines().enumerate().any(|(index, line)| {
        line_declares_local_callable(line, callee_name)
            && local_callable_body_has_call(caller_excerpt, index, callee_name)
    }) {
        LOCAL_CALLABLE_DECLARATION_BONUS
    } else {
        0.0
    }
}

fn local_callable_body_has_call(
    caller_excerpt: &str,
    declaration_index: usize,
    callee_name: &str,
) -> bool {
    caller_excerpt
        .lines()
        .skip(declaration_index)
        .take(6)
        .any(|line| local_callable_line_has_body_call(line, callee_name))
}

fn local_callable_line_has_body_call(line: &str, callee_name: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    if line_declares_local_callable(line, callee_name) {
        return expression_callable_body_has_call(line);
    }
    if line.contains(callee_name) {
        return false;
    }
    line.contains('(')
        && !line.starts_with("for ")
        && !line.starts_with("if ")
        && !line.starts_with("while ")
        && !line.starts_with("switch ")
}

fn expression_callable_body_has_call(line: &str) -> bool {
    let Some((_, body)) = line.split_once("=>") else {
        return false;
    };
    let body = body.split(';').next().unwrap_or(body);
    body.contains('(')
}
