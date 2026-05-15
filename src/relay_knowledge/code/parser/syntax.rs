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
            });
        }
    }

    Ok(captures)
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
