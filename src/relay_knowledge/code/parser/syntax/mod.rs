//! Tree-sitter parsing and bounded capture extraction.

use std::{
    ops::ControlFlow,
    panic::{self, AssertUnwindSafe},
    time::{Duration, Instant},
};

use tree_sitter::{
    Node, ParseOptions, Parser, Query, QueryCursor, QueryCursorOptions, StreamingIterator,
};

use super::{
    super::{CodeIndexError, languages::LanguageSpec},
    nodes::{SyntaxRange, node_text, syntax_range},
};

#[derive(Debug, Clone)]
pub(super) struct TagCapture {
    pub(super) name: String,
    pub(super) capture_kind: String,
    pub(super) name_node: SyntaxRange,
    pub(super) target_node: SyntaxRange,
    pub(super) target_has_error: bool,
    pub(super) local_type_parameter: bool,
}

const SYNTAX_BASE_BUDGET: Duration = Duration::from_millis(100);
const SYNTAX_MAX_BUDGET: Duration = Duration::from_millis(750);
const SYNTAX_BUDGET_BYTES_PER_MILLISECOND: usize = 1_024;
const MIN_REPEATED_INITIALIZER_FRAGMENT_LINES: usize = 32;

pub(super) fn parse_tree(
    language: LanguageSpec,
    content: &str,
) -> Result<tree_sitter::Tree, CodeIndexError> {
    parse_tree_with_budget(language, content, syntax_stage_budget(content.len()))
}

fn parse_tree_with_budget(
    language: LanguageSpec,
    content: &str,
    budget: Duration,
) -> Result<tree_sitter::Tree, CodeIndexError> {
    reject_pathological_c_family_fragment(language.id, content)?;
    let mut parser = Parser::new();
    parser
        .set_language(&(language.language)())
        .map_err(|error| CodeIndexError::TreeSitter(error.to_string()))?;
    let deadline = Instant::now() + budget;
    let mut budget_exhausted = false;
    let mut progress = |_: &tree_sitter::ParseState| {
        if Instant::now() >= deadline {
            budget_exhausted = true;
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let bytes = content.as_bytes();
    let parsed = parser.parse_with_options(
        &mut |offset, _| bytes.get(offset..).unwrap_or_default(),
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if budget_exhausted {
        return Err(syntax_budget_error("parser", budget));
    }
    parsed.ok_or_else(|| CodeIndexError::TreeSitter("parser returned no tree".to_owned()))
}

fn reject_pathological_c_family_fragment(
    language_id: &str,
    content: &str,
) -> Result<(), CodeIndexError> {
    if !matches!(language_id, "c" | "cpp") {
        return Ok(());
    }
    let mut significant_lines = 0usize;
    let mut initializer_lines = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        significant_lines += 1;
        initializer_lines += usize::from(designated_initializer_fragment_line(trimmed));
    }
    if initializer_lines >= MIN_REPEATED_INITIALIZER_FRAGMENT_LINES
        && initializer_lines == significant_lines
    {
        return Err(CodeIndexError::TreeSitter(
            "repeated top-level designated initializer fragment exceeds the bounded parser shape"
                .to_owned(),
        ));
    }

    Ok(())
}

fn designated_initializer_fragment_line(line: &str) -> bool {
    line.starts_with('{')
        && (line.ends_with("},") || line.ends_with('}'))
        && line.contains('.')
        && line.contains('=')
}

pub(super) fn parse_tree_safely(
    language: LanguageSpec,
    content: &str,
) -> Result<tree_sitter::Tree, CodeIndexError> {
    match panic::catch_unwind(AssertUnwindSafe(|| parse_tree(language, content))) {
        Ok(result) => result,
        Err(_) => Err(CodeIndexError::TreeSitter(
            "parser panicked while parsing file".to_owned(),
        )),
    }
}

fn extract_tag_captures(
    language: LanguageSpec,
    root: Node<'_>,
    content: &str,
) -> Result<Vec<TagCapture>, CodeIndexError> {
    let query = Query::new(&(language.language)(), language.tags_query)
        .map_err(|error| CodeIndexError::TreeSitter(error.to_string()))?;
    let capture_names = query.capture_names().to_vec();
    let mut cursor = QueryCursor::new();
    let budget = syntax_stage_budget(content.len());
    let deadline = Instant::now() + budget;
    let mut budget_exhausted = false;
    let mut progress = |_: &tree_sitter::QueryCursorState| {
        if Instant::now() >= deadline {
            budget_exhausted = true;
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let mut matches = cursor.matches_with_options(
        &query,
        root,
        content.as_bytes(),
        QueryCursorOptions::new().progress_callback(&mut progress),
    );
    let mut captures = Vec::new();

    while {
        matches.advance();
        matches.get().is_some()
    } {
        let query_match = matches.get().expect("match is present");
        let mut name_capture = None;
        let mut primary_capture = None;
        for capture in query_match.captures {
            let capture_name = capture_names[capture.index as usize];
            if capture_name == "name" {
                name_capture = Some(capture.node);
            } else if capture_name.starts_with("definition.")
                || capture_name.starts_with("reference.")
            {
                primary_capture = Some((capture_name.to_owned(), capture.node));
            }
        }
        if let (Some(name_node), Some((capture_kind, target_node))) =
            (name_capture, primary_capture)
        {
            captures.push(TagCapture {
                name: node_text(content, name_node),
                capture_kind,
                name_node: syntax_range(name_node),
                target_node: syntax_range(target_node),
                target_has_error: target_node.has_error(),
                local_type_parameter: local_type_parameter_reference(
                    language.id,
                    content,
                    name_node,
                ),
            });
        }
    }

    drop(matches);
    if budget_exhausted {
        return Err(syntax_budget_error("query", budget));
    }

    Ok(captures)
}

fn syntax_stage_budget(content_len: usize) -> Duration {
    let size_millis = content_len.saturating_add(SYNTAX_BUDGET_BYTES_PER_MILLISECOND - 1)
        / SYNTAX_BUDGET_BYTES_PER_MILLISECOND;
    SYNTAX_BASE_BUDGET
        .saturating_add(Duration::from_millis(size_millis as u64))
        .min(SYNTAX_MAX_BUDGET)
}

fn syntax_budget_error(stage: &str, budget: Duration) -> CodeIndexError {
    CodeIndexError::TreeSitter(format!(
        "{stage} exceeded bounded {} ms syntax budget",
        budget.as_millis()
    ))
}

fn local_type_parameter_reference(language_id: &str, content: &str, node: Node<'_>) -> bool {
    if !matches!(language_id, "python" | "typescript" | "tsx") {
        return false;
    }
    let name = node_text(content, node);
    let mut current = node;
    for _ in 0..12 {
        let Some(parent) = current.parent() else {
            return false;
        };
        if type_parameters_node(parent).is_some_and(|type_parameters| {
            !node_contains(type_parameters, node)
                && type_parameters_contain_name(content, type_parameters, &name)
        }) {
            return true;
        }
        current = parent;
    }

    false
}

fn type_parameters_node(parent: Node<'_>) -> Option<Node<'_>> {
    parent.child_by_field_name("type_parameters").or_else(|| {
        let mut cursor = parent.walk();
        parent
            .children(&mut cursor)
            .find(|child| child.kind() == "type_parameters")
    })
}

fn type_parameters_contain_name(content: &str, type_parameters: Node<'_>, name: &str) -> bool {
    if type_parameters.kind() == "type_parameter" {
        return type_parameter_name(content, type_parameters)
            .is_some_and(|parameter_name| parameter_name == name);
    }
    let mut cursor = type_parameters.walk();
    type_parameters.children(&mut cursor).any(|child| {
        if child.kind() == "type_parameter" {
            return type_parameter_name(content, child)
                .is_some_and(|parameter_name| parameter_name == name);
        }
        matches!(child.kind(), "identifier" | "type_identifier")
            && node_text(content, child) == name
    })
}

fn type_parameter_name(content: &str, type_parameter: Node<'_>) -> Option<String> {
    type_parameter
        .child_by_field_name("name")
        .map(|name| node_text(content, name))
        .or_else(|| first_identifier_name(content, type_parameter))
}

fn first_identifier_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(current.kind(), "identifier" | "type_identifier") {
            return Some(node_text(content, current));
        }
        let mut cursor = current.walk();
        let children = current.children(&mut cursor).collect::<Vec<_>>();
        stack.extend(children.into_iter().rev());
    }

    None
}

fn node_contains(parent: Node<'_>, child: Node<'_>) -> bool {
    parent.start_byte() <= child.start_byte() && parent.end_byte() >= child.end_byte()
}

pub(super) fn extract_tag_captures_safely(
    language: LanguageSpec,
    root: Node<'_>,
    content: &str,
) -> Result<Vec<TagCapture>, CodeIndexError> {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        extract_tag_captures(language, root, content)
    })) {
        Ok(result) => result,
        Err(_) => Err(CodeIndexError::TreeSitter(
            "query extraction panicked while parsing file".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c_language() -> LanguageSpec {
        LanguageSpec {
            id: "c",
            language: || tree_sitter_c::LANGUAGE.into(),
            tags_query: "",
        }
    }

    #[test]
    fn syntax_budget_scales_with_content_and_stays_bounded() {
        assert_eq!(syntax_stage_budget(0), SYNTAX_BASE_BUDGET);
        assert!(syntax_stage_budget(64 * 1_024) > SYNTAX_BASE_BUDGET);
        assert_eq!(syntax_stage_budget(usize::MAX), SYNTAX_MAX_BUDGET);
    }

    #[test]
    fn parser_cancels_pathological_error_recovery_at_the_budget() {
        let fragment = "(".repeat(64 * 1_024);

        let error = parse_tree_with_budget(c_language(), &fragment, Duration::ZERO)
            .expect_err("the progress callback should cancel pathological recovery");

        assert!(
            error
                .to_string()
                .contains("exceeded bounded 0 ms syntax budget")
        );
    }

    #[test]
    fn parser_rejects_repeated_top_level_initializer_fragments_before_grammar_recovery() {
        let mut fragment = String::new();
        for index in 0..MIN_REPEATED_INITIALIZER_FRAGMENT_LINES {
            fragment.push_str(&format!("{{ .flag = {index}, .value = 1 }},\n"));
        }

        let error = parse_tree(c_language(), &fragment)
            .expect_err("a repeated declaration-free initializer fragment should be bounded");

        assert!(
            error
                .to_string()
                .contains("top-level designated initializer fragment")
        );
    }

    #[test]
    fn parser_keeps_designated_initializers_inside_a_declaration() {
        let mut declaration = String::from("static const struct item values[] = {\n");
        for index in 0..MIN_REPEATED_INITIALIZER_FRAGMENT_LINES {
            declaration.push_str(&format!("    {{ .flag = {index}, .value = 1 }},\n"));
        }
        declaration.push_str("};\n");

        parse_tree(c_language(), &declaration)
            .expect("a declared initializer table remains eligible for structured parsing");
    }
}
