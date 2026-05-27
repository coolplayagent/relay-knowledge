use tree_sitter::Node;

use super::super::nodes::{
    SyntaxRange, first_named_child_of_kind, node_text, push_children_reverse, syntax_range,
};
use super::super::recovery::{
    decorated_function_error_body_is_statement_like, decorated_function_head_has_recoverable_tail,
    decorated_function_head_has_recovery_decorator, decorated_function_head_text,
};
use preprocessor::{LocalFunctionMacroDefinition, local_function_macro_definition};

mod gcc_recovery;
mod preprocessor;

use gcc_recovery::gcc_decorated_function_symbol;

const MAX_TOP_LEVEL_DATA_SYMBOL_LINES: usize = 80;

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "function_definition" => match macro_body_function_definition(content, node) {
            MacroBodyFunctionDefinition::Recovered(definition) => vec![definition],
            MacroBodyFunctionDefinition::Rejected => Vec::new(),
            MacroBodyFunctionDefinition::NotMacroBody => {
                if let Some(symbol) = gcc_decorated_function_symbol(content, node) {
                    return vec![symbol];
                }
                if let Some(symbol) = decorated_cpp_class_symbol(content, node) {
                    return vec![symbol];
                }
                if function_definition_has_unrecoverable_decorator_shape(content, node) {
                    return Vec::new();
                }
                if syntax_error_descendant(node) {
                    return Vec::new();
                }
                node.child_by_field_name("declarator")
                    .and_then(|declarator| declarator_name(content, declarator))
                    .map(|name| vec![(name, "function", syntax_range(node))])
                    .unwrap_or_default()
            }
        },
        "ERROR" if !has_ancestor_kind(node, "compound_statement") => {
            gcc_decorated_function_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "type_definition" if !has_ancestor_kind(node, "compound_statement") => {
            typedef_type_symbols(content, node)
        }
        "declaration" if !has_ancestor_kind(node, "compound_statement") => {
            if is_typedef_declaration(content, node) {
                typedef_type_symbols(content, node)
            } else {
                let mut symbols = enum_type_symbols(content, node);
                symbols.extend(function_declaration_symbols(content, node));
                symbols.extend(top_level_data_symbols(content, node));
                symbols
            }
        }
        "preproc_def" | "preproc_function_def" => node
            .child_by_field_name("name")
            .or_else(|| first_named_child_of_kind(node, "identifier"))
            .map(|name| vec![(node_text(content, name), "macro", syntax_range(node))])
            .unwrap_or_default(),
        "call_expression" if !has_ancestor_kind(node, "compound_statement") => {
            syscall_macro_definition(content, node)
                .or_else(|| macro_generated_function_definition(content, node))
                .map(|definition| vec![definition])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

enum MacroBodyFunctionDefinition {
    Recovered((String, &'static str, SyntaxRange)),
    Rejected,
    NotMacroBody,
}

fn function_definition_has_unrecoverable_decorator_shape(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    decorated_function_head_text(&text).is_some_and(|head| {
        decorated_function_head_has_recovery_decorator(head)
            && (!decorated_function_head_has_recoverable_tail(head, false, false, false)
                || !decorated_function_error_body_is_statement_like(&text))
    })
}

fn macro_body_function_definition(content: &str, node: Node<'_>) -> MacroBodyFunctionDefinition {
    let text = node_text(content, node);
    let Some(head) = text.split('{').next().map(str::trim) else {
        return MacroBodyFunctionDefinition::NotMacroBody;
    };
    let Some(macro_name) = head
        .split(|character: char| !c_identifier_char(character))
        .next()
        .filter(|name| uppercase_macro_token(name))
    else {
        return MacroBodyFunctionDefinition::NotMacroBody;
    };
    let after_name = head
        .get(macro_name.len()..)
        .map(str::trim_start)
        .unwrap_or_default();
    if !after_name.starts_with('(') {
        return MacroBodyFunctionDefinition::NotMacroBody;
    }
    let Some(arguments) = macro_body_argument_groups(head) else {
        return MacroBodyFunctionDefinition::Rejected;
    };
    let name = if definition_like_macro_name(macro_name) {
        let Some(name) = macro_generated_function_name_from_groups(&arguments, macro_name) else {
            return MacroBodyFunctionDefinition::Rejected;
        };
        name
    } else {
        match local_macro_generated_function_name(
            content,
            macro_name,
            &arguments,
            node.start_byte(),
        ) {
            LocalMacroFunctionName::Recovered(name) => name,
            LocalMacroFunctionName::FallbackDeclarator(name) => name,
            LocalMacroFunctionName::Rejected => return MacroBodyFunctionDefinition::Rejected,
            LocalMacroFunctionName::NotMacro => return MacroBodyFunctionDefinition::NotMacroBody,
        }
    };

    MacroBodyFunctionDefinition::Recovered((name, "function", syntax_range(node)))
}

fn macro_body_argument_groups(head: &str) -> Option<Vec<MacroArgument>> {
    let start = head.find('(')?;
    let end = head.rfind(')')?;
    if end <= start {
        return None;
    }

    Some(
        macro_argument_text_slots(&head[start..=end])
            .into_iter()
            .map(|argument| MacroArgument {
                text: argument.to_owned(),
                identifiers: macro_argument_text_identifiers(argument),
            })
            .collect(),
    )
}

fn enum_type_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut cursor = declaration.walk();
    declaration
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "enum_specifier")
        .filter(|child| node_text(content, *child).contains('{'))
        .filter_map(|child| {
            let name = child.child_by_field_name("name")?;
            let name = node_text(content, name);
            data_symbol_name(&name).then(|| (name, "type", syntax_range(child)))
        })
        .collect()
}

fn typedef_type_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let range = syntax_range(declaration);
    let mut cursor = declaration.walk();

    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| declarator_name(content, declarator))
        .filter(|name| data_symbol_name(name))
        .map(|name| (name, "type", range.clone()))
        .collect()
}

fn top_level_data_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let range = syntax_range(declaration);
    if range.line_end.saturating_sub(range.line_start) > MAX_TOP_LEVEL_DATA_SYMBOL_LINES {
        return Vec::new();
    }
    let composite_type = declaration_has_composite_type(content, declaration);
    let initializer_contract_type = declaration_has_initializer_contract_type(content, declaration);
    let mut cursor = declaration.walk();

    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| {
            initialized_data_declarator_name(
                content,
                declarator,
                composite_type,
                initializer_contract_type,
            )
        })
        .map(|name| (name, "constant", range.clone()))
        .collect()
}

fn initialized_data_declarator_name(
    content: &str,
    declarator: Node<'_>,
    composite_type: bool,
    initializer_contract_type: bool,
) -> Option<String> {
    if declarator.kind() != "init_declarator" {
        return None;
    }
    let value = declarator.child_by_field_name("value")?;
    if !matches!(value.kind(), "initializer_list" | "call_expression") {
        return None;
    }
    let inner = declarator.child_by_field_name("declarator")?;
    let array_declarator = contains_node_kind(inner, "array_declarator");
    let typedef_initializer = initializer_contract_type && value.kind() == "initializer_list";
    if !composite_type && !array_declarator && !typedef_initializer {
        return None;
    }
    if contains_node_kind(inner, "function_declarator") {
        return None;
    }

    declarator_name(content, inner).filter(|name| data_symbol_name(name))
}

fn declaration_has_composite_type(content: &str, declaration: Node<'_>) -> bool {
    declaration
        .child_by_field_name("type")
        .is_some_and(|type_node| {
            matches!(
                type_node.kind(),
                "struct_specifier" | "union_specifier" | "enum_specifier"
            ) || {
                let type_text = node_text(content, type_node);
                type_text.starts_with("struct ")
                    || type_text.starts_with("union ")
                    || type_text.starts_with("enum ")
            }
        })
}

fn declaration_has_initializer_contract_type(content: &str, declaration: Node<'_>) -> bool {
    declaration
        .child_by_field_name("type")
        .is_some_and(|type_node| typedef_like_contract_type(&node_text(content, type_node)))
}

fn typedef_like_contract_type(name: &str) -> bool {
    name.split_whitespace()
        .last()
        .is_some_and(c_external_contract_type_token)
}

fn c_external_contract_type_token(token: &str) -> bool {
    (token.ends_with("_t") && data_symbol_name(token))
        || (token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
            && token
                .chars()
                .any(|character| character.is_ascii_lowercase())
            && data_symbol_name(token))
}

fn data_symbol_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn c_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn c_declaration_prefix_token(token: &str) -> bool {
    matches!(
        token,
        "__always_inline"
            | "__attribute__"
            | "__attribute"
            | "__declspec"
            | "__declspec__"
            | "__inline"
            | "__inline__"
            | "always_inline"
            | "attribute"
            | "const"
            | "extern"
            | "inline"
            | "register"
            | "restrict"
            | "static"
            | "volatile"
    )
}

fn decorated_cpp_class_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let text = node_text(content, node);
    let head = text.split('{').next()?.trim();
    let tail = head.strip_prefix("class ")?;
    let declaration = tail.split(':').next().unwrap_or(tail);
    let name = declaration
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .rfind(|token| cpp_class_name_candidate(token))?;

    Some((name.to_owned(), "class", syntax_range(node)))
}

fn cpp_class_name_candidate(token: &str) -> bool {
    if token.is_empty() || matches!(token, "final") {
        return false;
    }
    let mut characters = token.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn syscall_macro_definition(
    content: &str,
    call: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let function = call.child_by_field_name("function")?;
    let macro_name = node_text(content, function);
    if !is_syscall_definition_macro(&macro_name) {
        return None;
    }
    let arguments = call.child_by_field_name("arguments")?;
    let syscall_name = first_named_child_of_kind(arguments, "identifier")?;

    Some((
        node_text(content, syscall_name),
        "function",
        syntax_range(call),
    ))
}

fn is_syscall_definition_macro(name: &str) -> bool {
    let Some(suffix) = name
        .strip_prefix("SYSCALL_DEFINE")
        .or_else(|| name.strip_prefix("COMPAT_SYSCALL_DEFINE"))
    else {
        return false;
    };

    !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
}

fn macro_generated_function_definition(
    content: &str,
    call: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let function = call.child_by_field_name("function")?;
    let macro_name = node_text(content, function);
    let range = macro_generated_definition_range(call);
    let arguments = call.child_by_field_name("arguments")?;
    let argument_groups = macro_argument_groups(content, arguments);
    let name = if definition_like_macro_name(&macro_name) {
        macro_generated_function_name_from_groups(&argument_groups, &macro_name)
    } else if range.has_following_body {
        match local_macro_generated_function_name(
            content,
            &macro_name,
            &argument_groups,
            call.start_byte(),
        ) {
            LocalMacroFunctionName::Recovered(name) => Some(name),
            LocalMacroFunctionName::FallbackDeclarator(name) => Some(name),
            LocalMacroFunctionName::Rejected | LocalMacroFunctionName::NotMacro => None,
        }
    } else {
        None
    }?;

    Some((name, "function", range.range))
}

struct MacroGeneratedRange {
    range: SyntaxRange,
    has_following_body: bool,
}

fn macro_generated_definition_range(call: Node<'_>) -> MacroGeneratedRange {
    let mut range = syntax_range(call);
    let has_following_body = call
        .next_named_sibling()
        .filter(|sibling| sibling.kind() == "compound_statement")
        .map(|body| {
            let body_range = syntax_range(body);
            range.byte_end = body_range.byte_end;
            range.line_end = body_range.line_end;
        })
        .is_some();

    MacroGeneratedRange {
        range,
        has_following_body,
    }
}

fn definition_like_macro_name(name: &str) -> bool {
    if matches!(name, "EXPORT_SYMBOL" | "EXPORT_SYMBOL_GPL" | "IS_ENABLED") {
        return false;
    }
    if is_syscall_definition_macro(name) {
        return true;
    }
    let tokens = name
        .split('_')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() || !uppercase_macro_token(name) {
        return false;
    }
    if tokens
        .iter()
        .any(|token| matches!(*token, "REGISTER" | "UNREGISTER"))
    {
        return false;
    }

    tokens
        .iter()
        .any(|token| matches!(*token, "HANDLER" | "FUNCTION" | "METHOD" | "CALLBACK"))
}

struct MacroArgument {
    text: String,
    identifiers: Vec<String>,
}

fn macro_generated_function_name_from_groups(
    argument_groups: &[MacroArgument],
    macro_name: &str,
) -> Option<String> {
    if declaration_style_macro_starts_with_return_type(macro_name, argument_groups) {
        return argument_groups
            .iter()
            .skip(1)
            .find_map(macro_argument_symbol_candidate);
    }

    argument_groups
        .iter()
        .find_map(macro_argument_symbol_candidate)
}

fn local_macro_generated_function_name(
    content: &str,
    macro_name: &str,
    argument_groups: &[MacroArgument],
    limit_byte: usize,
) -> LocalMacroFunctionName {
    let definition = match local_function_macro_definition(content, macro_name, limit_byte) {
        LocalFunctionMacroDefinition::Function(definition) => definition,
        LocalFunctionMacroDefinition::ActiveNonFunction => return LocalMacroFunctionName::Rejected,
        LocalFunctionMacroDefinition::Unavailable => {
            return LocalMacroFunctionName::FallbackDeclarator(macro_name.to_owned());
        }
        LocalFunctionMacroDefinition::Missing => return LocalMacroFunctionName::NotMacro,
    };
    let argument_index = macro_definition_function_name_parameter_index(
        &definition.replacement,
        &definition.parameters,
    );
    let Some(argument_index) = argument_index else {
        return LocalMacroFunctionName::Rejected;
    };
    let Some(argument) = argument_groups.get(argument_index) else {
        return LocalMacroFunctionName::Rejected;
    };

    match macro_argument_symbol_candidate(argument) {
        Some(name) => LocalMacroFunctionName::Recovered(name),
        None => LocalMacroFunctionName::Rejected,
    }
}

enum LocalMacroFunctionName {
    Recovered(String),
    FallbackDeclarator(String),
    Rejected,
    NotMacro,
}

fn macro_definition_function_name_parameter_index(
    replacement: &str,
    parameters: &[String],
) -> Option<usize> {
    parameters
        .iter()
        .position(|parameter| macro_replacement_parameter_is_function_name(replacement, parameter))
}

fn macro_replacement_parameter_is_function_name(replacement: &str, parameter: &str) -> bool {
    let mut search_start = 0usize;
    while let Some(relative_start) = replacement[search_start..].find(parameter) {
        let start = search_start + relative_start;
        let end = start + parameter.len();
        if identifier_boundary(replacement, start, end)
            && replacement[end..].trim_start().starts_with('(')
            && macro_replacement_head_looks_like_function_return(&replacement[..start])
        {
            return true;
        }
        search_start = end;
    }

    false
}

fn identifier_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();

    before.is_none_or(|character| !c_identifier_char(character))
        && after.is_none_or(|character| !c_identifier_char(character))
}

fn macro_replacement_head_looks_like_function_return(head: &str) -> bool {
    if head.contains('=') {
        return false;
    }
    let tokens = head
        .split(|character: char| !c_identifier_char(character))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let Some((return_type, prefixes)) = tokens.split_last() else {
        return false;
    };

    macro_replacement_return_type_token(return_type)
        && prefixes
            .iter()
            .all(|token| macro_replacement_declaration_prefix_token(token))
}

fn macro_replacement_return_type_token(token: &str) -> bool {
    c_macro_type_argument(token)
        || token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
}

fn macro_replacement_declaration_prefix_token(token: &str) -> bool {
    matches!(
        token,
        "__always_inline"
            | "__attribute__"
            | "__attribute"
            | "__declspec"
            | "__declspec__"
            | "__inline"
            | "__inline__"
            | "always_inline"
            | "attribute"
            | "const"
            | "extern"
            | "inline"
            | "register"
            | "restrict"
            | "static"
            | "volatile"
    ) || uppercase_macro_token(token)
}

fn macro_argument_groups(content: &str, arguments: Node<'_>) -> Vec<MacroArgument> {
    let text = node_text(content, arguments);
    macro_argument_text_slots(&text)
        .into_iter()
        .map(|argument| MacroArgument {
            text: argument.to_owned(),
            identifiers: macro_argument_text_identifiers(argument),
        })
        .collect()
}

fn macro_argument_text_slots(text: &str) -> Vec<&str> {
    let inner = text
        .trim()
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(text);
    let mut slots = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (index, character) in inner.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                slots.push(inner[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        slots.push(tail);
    }

    slots
}

fn declaration_style_macro_starts_with_return_type(
    macro_name: &str,
    argument_groups: &[MacroArgument],
) -> bool {
    if !declaration_style_macro_name(macro_name) || argument_groups.len() <= 1 {
        return false;
    }
    let Some(return_type) = argument_groups.first() else {
        return false;
    };
    let Some(symbol_argument) = argument_groups.get(1) else {
        return false;
    };

    macro_argument_looks_like_type(return_type)
        || (declaration_style_macro_uses_return_type_slot(macro_name)
            && macro_argument_looks_like_custom_return_type(return_type)
            && macro_argument_symbol_candidate(symbol_argument).is_some()
            && !macro_argument_looks_like_type(symbol_argument))
}

fn macro_argument_text_identifiers(argument: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    let mut start = None;
    for (index, character) in argument.char_indices() {
        if character == '_' || character.is_ascii_alphanumeric() {
            start.get_or_insert(index);
            continue;
        }
        if let Some(identifier_start) = start.take() {
            push_macro_argument_identifier(argument, identifier_start, index, &mut identifiers);
        }
    }
    if let Some(identifier_start) = start {
        push_macro_argument_identifier(
            argument,
            identifier_start,
            argument.len(),
            &mut identifiers,
        );
    }

    identifiers
}

fn push_macro_argument_identifier(
    argument: &str,
    start: usize,
    end: usize,
    identifiers: &mut Vec<String>,
) {
    let identifier = &argument[start..end];
    if identifier
        .chars()
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
    {
        identifiers.push(identifier.to_owned());
    }
}

fn declaration_style_macro_name(name: &str) -> bool {
    let tokens = name
        .split('_')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.iter().any(|token| matches!(*token, "DECLARE"))
        && tokens
            .iter()
            .any(|token| matches!(*token, "FUNCTION" | "METHOD" | "CALLBACK"))
}

fn declaration_style_macro_uses_return_type_slot(name: &str) -> bool {
    name.split('_')
        .filter(|token| !token.is_empty())
        .any(|token| matches!(token, "FUNCTION" | "METHOD"))
}

fn macro_argument_looks_like_type(argument: &MacroArgument) -> bool {
    let trimmed = argument.text.trim();
    trimmed.contains('*')
        || trimmed.contains("struct ")
        || trimmed.contains("union ")
        || trimmed.contains("enum ")
        || c_macro_type_argument(trimmed)
        || argument
            .identifiers
            .iter()
            .any(|name| c_macro_type_argument(name))
}

fn macro_argument_looks_like_custom_return_type(argument: &MacroArgument) -> bool {
    argument.identifiers.len() == 1
        && argument.identifiers[0]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
}

fn macro_argument_symbol_candidate(argument: &MacroArgument) -> Option<String> {
    argument
        .identifiers
        .iter()
        .find(|name| {
            data_symbol_name(name) && !uppercase_macro_token(name) && !c_macro_type_argument(name)
        })
        .cloned()
}

fn c_macro_type_argument(name: &str) -> bool {
    matches!(
        name,
        "void"
            | "char"
            | "short"
            | "int"
            | "long"
            | "float"
            | "double"
            | "signed"
            | "unsigned"
            | "const"
            | "volatile"
            | "static"
            | "extern"
            | "inline"
            | "struct"
            | "union"
            | "enum"
    ) || name.ends_with("_t")
}

fn uppercase_macro_token(name: &str) -> bool {
    name.chars().all(|character| {
        character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
    }) && name.chars().any(|character| character.is_ascii_uppercase())
}

fn function_declaration_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut cursor = declaration.walk();
    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| {
            let function_declarator = direct_function_declarator(declarator)?;
            let name = declarator_name(content, function_declarator)?;

            Some((name, "function_declaration", syntax_range(declaration)))
        })
        .collect()
}

fn is_typedef_declaration(content: &str, declaration: Node<'_>) -> bool {
    let mut stack = vec![declaration];
    while let Some(node) = stack.pop() {
        if node.kind() == "storage_class_specifier" && node_text(content, node) == "typedef" {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}

fn direct_function_declarator(declarator: Node<'_>) -> Option<Node<'_>> {
    let mut stack = vec![declarator];
    while let Some(node) = stack.pop() {
        if node.kind() == "parameter_declaration" {
            continue;
        }
        if node.kind() == "function_declarator" && !is_function_pointer_variable(node) {
            return Some(node);
        }
        push_children_reverse(node, &mut stack);
    }

    None
}

fn is_function_pointer_variable(function_declarator: Node<'_>) -> bool {
    function_declarator
        .child_by_field_name("declarator")
        .is_some_and(has_parenthesized_pointer_declarator)
}

fn declarator_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "identifier" | "field_identifier" | "type_identifier"
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

fn contains_node_kind(root: Node<'_>, kind: &str) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}

fn has_ancestor_kind(mut node: Node<'_>, kind: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return true;
        }
        node = parent;
    }

    false
}

fn syntax_error_descendant(root: Node<'_>) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() || node.kind() == "ERROR" {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }
    false
}

fn has_parenthesized_pointer_declarator(root: Node<'_>) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "parenthesized_declarator"
            && contains_node_kind(node, "pointer_declarator")
        {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_macro_recovery_distinguishes_unknown_and_non_function_macros() {
        let arguments = vec![MacroArgument {
            text: "ngx_http_demo_access".to_owned(),
            identifiers: vec!["ngx_http_demo_access".to_owned()],
        }];

        assert!(matches!(
            local_macro_generated_function_name("", "NGX_HTTP_DEMO", &arguments, 0,),
            LocalMacroFunctionName::NotMacro
        ));

        let content = "#define MODULE_ACCESS_PHASE(name) name\n";
        assert!(matches!(
            local_macro_generated_function_name(
                content,
                "MODULE_ACCESS_PHASE",
                &arguments,
                content.len(),
            ),
            LocalMacroFunctionName::Rejected
        ));
    }
}
