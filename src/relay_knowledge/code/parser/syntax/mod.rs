//! Tree-sitter parsing and bounded capture extraction.

use std::{
    cell::RefCell,
    collections::HashMap,
    ops::ControlFlow,
    panic::{self, AssertUnwindSafe},
    sync::{Arc, Mutex, OnceLock},
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
    pub(super) doc_owner_node: SyntaxRange,
    pub(super) target_has_error: bool,
    pub(super) local_type_parameter: bool,
}

const SYNTAX_BASE_WORK_QUANTA: usize = 4_096;
const SYNTAX_MAX_WORK_QUANTA: usize = 32_768;
const SYNTAX_BUDGET_BYTES_PER_WORK_QUANTUM: usize = 24;
const MIN_REPEATED_INITIALIZER_FRAGMENT_LINES: usize = 32;
const MAX_CACHED_SYNTAX_PARSERS: usize = 64;
const MAX_CACHED_TAG_QUERIES: usize = 64;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct SyntaxParserCacheKey {
    language_id: &'static str,
    language_factory_address: usize,
}

#[derive(Default)]
struct SyntaxParserCache {
    parsers: HashMap<SyntaxParserCacheKey, Box<Parser>>,
}

impl SyntaxParserCache {
    fn get_or_try_insert(
        &mut self,
        key: SyntaxParserCacheKey,
        create: impl FnOnce() -> Result<Parser, CodeIndexError>,
    ) -> Result<Option<&mut Parser>, CodeIndexError> {
        let at_capacity = self.parsers.len() >= MAX_CACHED_SYNTAX_PARSERS;
        match self.parsers.entry(key) {
            std::collections::hash_map::Entry::Occupied(parser) => {
                Ok(Some(parser.into_mut().as_mut()))
            }
            std::collections::hash_map::Entry::Vacant(_) if at_capacity => Ok(None),
            std::collections::hash_map::Entry::Vacant(parser) => {
                Ok(Some(parser.insert(Box::new(create()?)).as_mut()))
            }
        }
    }
}

thread_local! {
    static SYNTAX_PARSERS: RefCell<SyntaxParserCache> =
        RefCell::new(SyntaxParserCache::default());
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct TagQueryCacheKey {
    language_id: &'static str,
    language_factory_address: usize,
    // Static query identity keeps lookup constant-time without hashing the
    // complete capture query and separates test-only alternate specifications.
    query_address: usize,
    query_len: usize,
}

type CompiledTagQueryCache = HashMap<TagQueryCacheKey, Arc<Query>>;

static COMPILED_TAG_QUERIES: OnceLock<Mutex<CompiledTagQueryCache>> = OnceLock::new();

struct SyntaxCallbackWorkBudget {
    remaining_quanta: usize,
    exhausted: bool,
}

impl SyntaxCallbackWorkBudget {
    fn new(work_quanta: usize) -> Self {
        Self {
            remaining_quanta: work_quanta,
            exhausted: false,
        }
    }

    fn consume(&mut self) -> ControlFlow<()> {
        if self.remaining_quanta == 0 {
            self.exhausted = true;
            return ControlFlow::Break(());
        }
        self.remaining_quanta -= 1;
        ControlFlow::Continue(())
    }
}

pub(super) fn parse_tree(
    language: LanguageSpec,
    content: &str,
) -> Result<tree_sitter::Tree, CodeIndexError> {
    parse_tree_with_budget(language, content, syntax_stage_work_quanta(content.len()))
}

fn parse_tree_with_budget(
    language: LanguageSpec,
    content: &str,
    work_quanta: usize,
) -> Result<tree_sitter::Tree, CodeIndexError> {
    reject_pathological_c_family_fragment(language.id, content)?;
    let mut work_budget = SyntaxCallbackWorkBudget::new(work_quanta);
    let mut progress = |_: &tree_sitter::ParseState| work_budget.consume();
    let bytes = content.as_bytes();
    let parsed = with_syntax_parser(language, |parser| {
        parser.parse_with_options(
            &mut |offset, _| bytes.get(offset..).unwrap_or_default(),
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
    })?;
    if work_budget.exhausted {
        return Err(syntax_budget_error("parser", work_quanta));
    }
    parsed.ok_or_else(|| CodeIndexError::TreeSitter("parser returned no tree".to_owned()))
}

fn with_syntax_parser<T>(
    language: LanguageSpec,
    operation: impl FnOnce(&mut Parser) -> T,
) -> Result<T, CodeIndexError> {
    let key = SyntaxParserCacheKey {
        language_id: language.id,
        language_factory_address: language.language as usize,
    };
    SYNTAX_PARSERS.with(|parsers| {
        let mut parsers = parsers.borrow_mut();
        if let Some(parser) =
            parsers.get_or_try_insert(key, || configured_syntax_parser(language))?
        {
            parser.reset();
            return Ok(operation(parser));
        }

        drop(parsers);
        let mut parser = configured_syntax_parser(language)?;
        parser.reset();
        Ok(operation(&mut parser))
    })
}

fn configured_syntax_parser(language: LanguageSpec) -> Result<Parser, CodeIndexError> {
    let mut parser = Parser::new();
    parser
        .set_language(&(language.language)())
        .map_err(|error| CodeIndexError::TreeSitter(error.to_string()))?;
    Ok(parser)
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
    let query = compiled_tag_query(language)?;
    let capture_names = query.capture_names().to_vec();
    let mut cursor = QueryCursor::new();
    let work_quanta = syntax_stage_work_quanta(content.len());
    let mut work_budget = SyntaxCallbackWorkBudget::new(work_quanta);
    let mut progress = |_: &tree_sitter::QueryCursorState| work_budget.consume();
    let mut matches = cursor.matches_with_options(
        query.as_ref(),
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
            let doc_owner_node = capture_doc_owner_node(language.id, &capture_kind, target_node);
            captures.push(TagCapture {
                name: node_text(content, name_node),
                capture_kind,
                name_node: syntax_range(name_node),
                target_node: syntax_range(target_node),
                doc_owner_node: syntax_range(doc_owner_node),
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
    if work_budget.exhausted {
        return Err(syntax_budget_error("query", work_quanta));
    }

    Ok(captures)
}

fn capture_doc_owner_node<'tree>(
    language_id: &str,
    capture_kind: &str,
    target_node: Node<'tree>,
) -> Node<'tree> {
    if !matches!(language_id, "c" | "cpp") || !capture_kind.starts_with("definition.") {
        return target_node;
    }

    let mut cursor = target_node;
    let mut declaration = None;
    while let Some(parent) = cursor.parent() {
        if matches!(
            parent.kind(),
            "declaration" | "field_declaration" | "friend_declaration" | "function_definition"
        ) {
            declaration = Some(parent);
            break;
        }
        if matches!(
            parent.kind(),
            "class_specifier" | "namespace_definition" | "translation_unit"
        ) {
            break;
        }
        cursor = parent;
    }

    let mut owner = declaration.unwrap_or(target_node);
    while let Some(parent) = owner.parent() {
        if parent.kind() != "template_declaration" {
            break;
        }
        owner = parent;
    }
    owner
}

fn compiled_tag_query(language: LanguageSpec) -> Result<Arc<Query>, CodeIndexError> {
    let key = tag_query_cache_key(language);
    {
        let queries = lock_compiled_tag_queries();
        if let Some(query) = queries.get(&key) {
            return Ok(Arc::clone(query));
        }
    }

    let query = Arc::new(
        Query::new(&(language.language)(), language.tags_query)
            .map_err(|error| CodeIndexError::TreeSitter(error.to_string()))?,
    );
    let mut queries = lock_compiled_tag_queries();
    if let Some(existing) = queries.get(&key) {
        return Ok(Arc::clone(existing));
    }
    if queries.len() < MAX_CACHED_TAG_QUERIES {
        queries.insert(key, Arc::clone(&query));
    }

    Ok(query)
}

fn tag_query_cache_key(language: LanguageSpec) -> TagQueryCacheKey {
    TagQueryCacheKey {
        language_id: language.id,
        language_factory_address: language.language as usize,
        query_address: language.tags_query.as_ptr() as usize,
        query_len: language.tags_query.len(),
    }
}

fn lock_compiled_tag_queries() -> std::sync::MutexGuard<'static, CompiledTagQueryCache> {
    match COMPILED_TAG_QUERIES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        Ok(queries) => queries,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn syntax_stage_work_quanta(content_len: usize) -> usize {
    let size_quanta = content_len.saturating_add(SYNTAX_BUDGET_BYTES_PER_WORK_QUANTUM - 1)
        / SYNTAX_BUDGET_BYTES_PER_WORK_QUANTUM;
    SYNTAX_BASE_WORK_QUANTA
        .saturating_add(size_quanta)
        .min(SYNTAX_MAX_WORK_QUANTA)
}

fn syntax_budget_error(stage: &str, work_quanta: usize) -> CodeIndexError {
    CodeIndexError::TreeSitter(format!(
        "{stage} exceeded bounded syntax budget of {work_quanta} callback work quanta"
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
#[path = "mod_tests.rs"]
mod tests;
