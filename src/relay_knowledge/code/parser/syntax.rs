use std::panic::{self, AssertUnwindSafe};

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

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

pub(super) fn parse_tree(
    language: LanguageSpec,
    content: &str,
) -> Result<tree_sitter::Tree, CodeIndexError> {
    let mut parser = Parser::new();
    parser
        .set_language(&(language.language)())
        .map_err(|error| CodeIndexError::TreeSitter(error.to_string()))?;
    parser
        .parse(content, None)
        .ok_or_else(|| CodeIndexError::TreeSitter("parser returned no tree".to_owned()))
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
    let mut matches = cursor.matches(&query, root, content.as_bytes());
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

    Ok(captures)
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
