use tree_sitter::Node;

use super::nodes::{SyntaxRange, node_text, push_children_reverse, syntax_range};
use super::recovery::{
    code_contains_char, decorated_function_error_body_is_statement_like,
    decorated_function_head_has_recoverable_tail, decorated_function_head_text,
    scan_code_line_indices, token_starts_in_angle_arguments,
};

pub(super) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "class_specifier" | "enum_specifier" | "struct_specifier" | "union_specifier"
            if cpp_type_declaration_context(content, node) =>
        {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "declaration" => decorated_cpp_declaration_type_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "ERROR" if decorated_declaration_head_starts_with_type_definition(content, node) => {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "ERROR" => gcc_decorated_function_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "function_definition"
            if decorated_declaration_head_starts_with_type_definition(content, node)
                && (node.child_by_field_name("declarator").is_none()
                    || !decorated_declaration_head_declares_function(content, node)) =>
        {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "function_definition" if cpp_function_definition_is_destructor(content, node) => Vec::new(),
        "function_definition" if node.has_error() => gcc_decorated_function_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "function_definition" => node
            .child_by_field_name("declarator")
            .and_then(|declarator| declarator_name(content, declarator))
            .or_else(|| gcc_decorated_function_name(content, node))
            .map(|name| vec![(name, "function", syntax_range(node))])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn decorated_cpp_declaration_type_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let type_node = direct_definition_type_specifier(content, node)?;
    decorated_cpp_type_symbol(content, type_node)
        .or_else(|| decorated_cpp_type_symbol(content, node))
        .map(|(name, kind, _)| (name, kind, syntax_range(node)))
}

fn direct_definition_type_specifier<'tree>(
    content: &str,
    node: Node<'tree>,
) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| {
            matches!(
                child.kind(),
                "class_specifier" | "enum_specifier" | "struct_specifier" | "union_specifier"
            ) && cpp_type_declaration_context(content, *child)
        })
        .or_else(|| {
            decorated_declaration_head_starts_with_type_definition(content, node).then_some(node)
        })
}

fn cpp_type_declaration_context(content: &str, node: Node<'_>) -> bool {
    if node_text(content, node).contains('{') {
        return true;
    }
    let Some(parent) = node
        .parent()
        .filter(|parent| parent.kind() == "declaration")
    else {
        return false;
    };
    content
        .get(node.end_byte()..parent.end_byte())
        .is_some_and(|trailing| trailing.trim() == ";")
}

fn decorated_declaration_head_starts_with_type_definition(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    if !text.contains('{') {
        return false;
    }
    let head = text
        .split(['{', ';'])
        .next()
        .unwrap_or(text.as_str())
        .trim();
    for token in cpp_head_tokens(head) {
        if cpp_type_intro_keyword(token.text) {
            return true;
        }
        if cpp_declaration_prefix_token(token.text) {
            continue;
        }
        return false;
    }

    false
}

fn decorated_declaration_head_declares_function(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    let head = text
        .split(['{', ';'])
        .next()
        .unwrap_or(text.as_str())
        .trim();
    let tokens = cpp_head_tokens(head);
    for (index, token) in tokens.iter().enumerate() {
        if !cpp_type_intro_keyword(token.text) {
            continue;
        }
        let Some(name) = cpp_type_name_after_intro_token(head, &tokens[index + 1..]) else {
            return false;
        };
        return head[name.end..].contains('(');
    }

    false
}

fn decorated_cpp_type_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let text = node_text(content, node);
    let head = text
        .split(['{', ';'])
        .next()
        .unwrap_or(text.as_str())
        .trim();
    let tokens = cpp_head_tokens(head);
    for (index, token) in tokens.iter().enumerate() {
        let kind = match token.text {
            "class" => "class",
            "struct" | "union" | "enum" => "type",
            _ => continue,
        };
        let name = cpp_type_name_after_intro(head, &tokens[index + 1..])?;
        return Some((name.to_owned(), kind, syntax_range(node)));
    }

    None
}

#[derive(Clone, Copy)]
struct CppHeadToken<'text> {
    text: &'text str,
    start: usize,
    end: usize,
}

fn cpp_head_tokens(head: &str) -> Vec<CppHeadToken<'_>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    for (index, character) in head.char_indices() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = token_start.take() {
            tokens.push(CppHeadToken {
                text: &head[start..index],
                start,
                end: index,
            });
        }
    }
    if let Some(start) = token_start {
        tokens.push(CppHeadToken {
            text: &head[start..],
            start,
            end: head.len(),
        });
    }

    tokens
}

fn cpp_type_name_after_intro<'text>(
    head: &'text str,
    tokens: &[CppHeadToken<'text>],
) -> Option<&'text str> {
    cpp_type_name_after_intro_token(head, tokens).map(|token| token.text)
}

fn cpp_type_name_after_intro_token<'text>(
    head: &'text str,
    tokens: &[CppHeadToken<'text>],
) -> Option<CppHeadToken<'text>> {
    let mut index = cpp_skip_type_name_prefix(tokens);
    while tokens
        .get(index)
        .is_some_and(|token| matches!(token.text, "class" | "struct"))
    {
        index += 1;
    }

    let mut name = *tokens.get(index)?;
    if !cpp_type_name_candidate(name.text) {
        return None;
    }

    while let Some(next) = tokens.get(index + 1) {
        if !cpp_type_name_candidate(next.text) || !cpp_tokens_joined_by_qualifier(head, name, *next)
        {
            break;
        }
        name = *next;
        index += 1;
    }

    Some(name)
}

fn cpp_skip_type_name_prefix(tokens: &[CppHeadToken<'_>]) -> usize {
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        if cpp_type_name_decorator_prefix(token.text) {
            index += 1;
            while tokens
                .get(index)
                .is_some_and(|payload| cpp_decorator_payload_token(payload.text))
            {
                index += 1;
            }
            continue;
        }
        if cpp_decorator_payload_token(token.text) {
            index += 1;
            continue;
        }
        break;
    }

    index
}

fn cpp_tokens_joined_by_qualifier(
    head: &str,
    left: CppHeadToken<'_>,
    right: CppHeadToken<'_>,
) -> bool {
    let separator = &head[left.end..right.start];
    separator.contains("::")
        && separator
            .chars()
            .all(|character| character == ':' || character.is_ascii_whitespace())
}

fn cpp_type_name_candidate(token: &str) -> bool {
    if cpp_keyword_token(token) {
        return false;
    }
    if cpp_builtin_type_token(token) {
        return false;
    }
    let mut characters = token.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn cpp_type_intro_keyword(token: &str) -> bool {
    matches!(token, "class" | "struct" | "union" | "enum")
}

fn cpp_declaration_prefix_token(token: &str) -> bool {
    cpp_decorator_token(token)
        || cpp_decorator_payload_token(token)
        || matches!(
            token,
            "__always_inline"
                | "__inline"
                | "__inline__"
                | "alignas"
                | "const"
                | "constexpr"
                | "export"
                | "extern"
                | "friend"
                | "inline"
                | "static"
                | "template"
                | "typename"
                | "using"
                | "volatile"
        )
}

fn cpp_type_name_decorator_prefix(token: &str) -> bool {
    cpp_double_underscore_decorator_token(token)
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
}

fn cpp_decorator_payload_token(token: &str) -> bool {
    matches!(
        token,
        "always_inline"
            | "annotate"
            | "dllimport"
            | "dllexport"
            | "visibility"
            | "default"
            | "hidden"
    )
}

fn cpp_keyword_token(token: &str) -> bool {
    matches!(
        token,
        "alignas"
            | "class"
            | "const"
            | "constexpr"
            | "enum"
            | "explicit"
            | "export"
            | "extern"
            | "final"
            | "friend"
            | "inline"
            | "mutable"
            | "namespace"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "struct"
            | "template"
            | "typename"
            | "union"
            | "using"
            | "virtual"
            | "volatile"
    )
}

fn cpp_builtin_type_token(token: &str) -> bool {
    matches!(
        token,
        "auto"
            | "bool"
            | "char"
            | "char8_t"
            | "char16_t"
            | "char32_t"
            | "double"
            | "float"
            | "int"
            | "long"
            | "short"
            | "signed"
            | "unsigned"
            | "void"
            | "wchar_t"
    )
}

fn cpp_decorator_token(token: &str) -> bool {
    cpp_double_underscore_decorator_token(token)
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
        || (token.chars().any(|character| character == '_')
            && token.chars().all(|character| {
                character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
            }))
}

fn cpp_double_underscore_decorator_token(token: &str) -> bool {
    matches!(
        token,
        "__attribute__" | "__attribute" | "__declspec" | "__declspec__" | "attribute"
    )
}

fn gcc_decorated_function_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    gcc_decorated_function_name(content, node).map(|name| (name, "function", syntax_range(node)))
}

fn cpp_function_definition_is_destructor(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    let Some(head) = decorated_function_head_text(&text) else {
        return false;
    };
    let Some(parameter_start) = cpp_top_level_parameter_start(head) else {
        return false;
    };
    head[..parameter_start]
        .trim_end()
        .rsplit("::")
        .next()
        .is_some_and(|tail| tail.trim_start().starts_with('~'))
}

fn gcc_decorated_function_name(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node);
    if !text.contains('{') {
        return None;
    }
    if !decorated_function_error_body_is_statement_like(&text) {
        return None;
    }
    let head = decorated_function_head_text(&text)?;
    if !cpp_function_head_has_recovery_decorator(head) {
        return None;
    }
    if !decorated_function_head_has_recoverable_tail(head, true, true, true) {
        return None;
    }
    if head.contains("::~") {
        return None;
    }
    let (name_start, name_end) = cpp_function_name_bounds_from_decorated_head(head)?;
    let name = &head[name_start..name_end];
    if !cpp_function_name_candidate(name) || !cpp_function_head_is_declaration(head, name) {
        return None;
    }

    Some(name.to_owned())
}

fn cpp_function_head_has_recovery_decorator(head: &str) -> bool {
    let Some(parameter_start) = cpp_top_level_parameter_start(head) else {
        return false;
    };
    let parameter_end = cpp_closing_parenthesis_index(&head[parameter_start..])
        .map_or(head.len(), |index| parameter_start + index + 1);
    cpp_head_tokens(&head[..parameter_start])
        .iter()
        .chain(cpp_head_tokens(&head[parameter_end..]).iter())
        .any(|token| {
            cpp_decorator_token(token.text)
                || matches!(
                    token.text,
                    "__always_inline" | "__inline" | "__inline__" | "always_inline"
                )
        })
}

fn cpp_function_name_bounds_from_decorated_head(head: &str) -> Option<(usize, usize)> {
    let parameter_start = cpp_top_level_parameter_start(head)?;
    cpp_name_bounds_before_open(head, parameter_start)
}

fn cpp_top_level_parameter_start(head: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut candidate = None;
    let literals_closed = scan_code_line_indices(head, |index, character| match character {
        '(' => {
            if depth == 0 && cpp_parameter_open_looks_like_declarator(head, index) {
                candidate = Some(index);
            }
            depth += 1;
        }
        ')' => depth = depth.saturating_sub(1),
        _ => {}
    });

    literals_closed.then_some(candidate).flatten()
}

fn cpp_parameter_open_looks_like_declarator(head: &str, parameter_start: usize) -> bool {
    let Some((name_start, name_end)) = cpp_name_bounds_before_open(head, parameter_start) else {
        return false;
    };
    let token = &head[name_start..name_end];
    cpp_function_name_candidate(token) && !cpp_declaration_prefix_token(token)
}

fn cpp_name_bounds_before_open(head: &str, parameter_start: usize) -> Option<(usize, usize)> {
    if let Some(bounds) = cpp_operator_bounds_before_open(head, parameter_start) {
        return Some(bounds);
    }
    let name_end = head[..parameter_start].trim_end().len();
    let name_start = head[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !(character.is_ascii_alphanumeric() || *character == '_'))
        .map_or(0, |(index, character)| index + character.len_utf8());
    if head[..name_start].trim_end().ends_with('~') {
        return None;
    }
    (name_start < name_end).then_some((name_start, name_end))
}

fn cpp_operator_bounds_before_open(head: &str, parameter_start: usize) -> Option<(usize, usize)> {
    let name_end = head[..parameter_start].trim_end().len();
    let prefix = &head[..name_end];
    let operator_start = prefix.rfind("operator")?;
    if prefix[..operator_start]
        .chars()
        .next_back()
        .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    let suffix = prefix[operator_start + "operator".len()..].trim();
    (cpp_punctuation_operator_suffix(suffix) || cpp_conversion_operator_suffix(suffix))
        .then_some((operator_start, name_end))
}

fn cpp_punctuation_operator_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix
            .chars()
            .all(|character| character.is_ascii_punctuation() || character.is_ascii_whitespace())
}

fn cpp_conversion_operator_suffix(suffix: &str) -> bool {
    let tokens = cpp_head_tokens(suffix);
    !tokens.is_empty()
        && tokens.iter().all(|token| {
            cpp_declaration_prefix_token(token.text)
                || cpp_type_name_candidate(token.text)
                || cpp_builtin_type_token(token.text)
        })
        && suffix.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, '_' | ':' | '*' | '&' | '<' | '>' | ',')
        })
}

fn cpp_function_head_is_declaration(head: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let Some((name_start, name_end)) = cpp_function_name_bounds_from_decorated_head(head) else {
        return false;
    };
    if &head[name_start..name_end] != name {
        return false;
    }
    if code_contains_char(&head[..name_start], '=') {
        return false;
    }
    let prefix = cpp_strip_trailing_qualified_declarator_scope(&head[..name_start]);
    let tokens = cpp_head_tokens(prefix);
    if cpp_conversion_operator_name(name) {
        return tokens
            .iter()
            .all(|token| cpp_declaration_prefix_token(token.text));
    }
    let Some(type_index) = tokens.iter().rposition(|token| {
        !cpp_declaration_prefix_token(token.text)
            && !token_starts_in_angle_arguments(prefix, token.start)
    }) else {
        return false;
    };
    if !cpp_function_return_type_at(prefix, &tokens, type_index) {
        return false;
    }
    let type_start = cpp_qualified_return_type_start(prefix, &tokens, type_index);

    tokens[..type_start].iter().all(|token| {
        cpp_declaration_prefix_token(token.text) || cpp_function_return_type_token(token.text)
    })
}

fn cpp_closing_parenthesis_index(text: &str) -> Option<usize> {
    if !text.starts_with('(') {
        return None;
    }
    let mut depth = 0isize;
    let mut matched_end = None;
    let literals_closed = scan_code_line_indices(text, |index, character| {
        if matched_end.is_some() {
            return;
        }
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    matched_end = Some(index);
                }
            }
            _ => {}
        }
    });
    literals_closed.then_some(matched_end).flatten()
}

fn cpp_strip_trailing_qualified_declarator_scope(prefix: &str) -> &str {
    let mut cursor = prefix.trim_end().len();
    let mut stripped_scope = false;
    loop {
        let before_colons = prefix[..cursor].trim_end().len();
        if !prefix[..before_colons].ends_with("::") {
            break;
        }
        let scope_end = before_colons.saturating_sub(2);
        let ident_end = prefix[..scope_end].trim_end().len();
        let ident_start = prefix[..ident_end]
            .char_indices()
            .rev()
            .find(|(_, character)| !(character.is_ascii_alphanumeric() || *character == '_'))
            .map_or(0, |(index, character)| index + character.len_utf8());
        if ident_start >= ident_end {
            break;
        }
        cursor = ident_start;
        stripped_scope = true;
    }

    if stripped_scope {
        &prefix[..cursor]
    } else {
        prefix
    }
}

fn cpp_function_return_type_at(head: &str, tokens: &[CppHeadToken<'_>], type_index: usize) -> bool {
    let token = tokens[type_index].text;
    if cpp_function_return_type_token(token) {
        return true;
    }
    cpp_qualified_return_type_start(head, tokens, type_index) < type_index
        && cpp_type_name_candidate(token)
}

fn cpp_qualified_return_type_start(
    head: &str,
    tokens: &[CppHeadToken<'_>],
    type_index: usize,
) -> usize {
    let mut start = type_index;
    while start > 0
        && cpp_type_name_candidate(tokens[start - 1].text)
        && cpp_tokens_joined_by_qualifier(head, tokens[start - 1], tokens[start])
    {
        start -= 1;
    }
    start
}

fn cpp_function_name_candidate(name: &str) -> bool {
    cpp_type_name_candidate(name) || name.starts_with("operator")
}

fn cpp_conversion_operator_name(name: &str) -> bool {
    name.strip_prefix("operator").is_some_and(|suffix| {
        suffix
            .trim_start()
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
    })
}

fn cpp_function_return_type_token(token: &str) -> bool {
    cpp_builtin_type_token(token)
        || (token.ends_with("_t") && cpp_type_name_candidate(token))
        || (token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
            && token
                .chars()
                .any(|character| character.is_ascii_lowercase())
            && cpp_type_name_candidate(token))
}

fn declarator_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "qualified_identifier" {
            return terminal_qualified_identifier_name(content, current);
        }
        if matches!(
            current.kind(),
            "identifier" | "field_identifier" | "operator_name"
        ) {
            return Some(node_text(content, current));
        }
        if let Some(declarator) = current.child_by_field_name("declarator") {
            stack.push(declarator);
            continue;
        }
        push_children_reverse(current, &mut stack);
    }

    None
}

fn terminal_qualified_identifier_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    let mut terminal = None;
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "identifier" | "field_identifier" | "operator_name"
        ) && terminal
            .as_ref()
            .is_none_or(|(start, _)| current.start_byte() >= *start)
        {
            terminal = Some((current.start_byte(), node_text(content, current)));
        }
        push_children_reverse(current, &mut stack);
    }

    terminal.map(|(_, name)| name)
}
