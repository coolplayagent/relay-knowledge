//! Function symbols produced by C macro bodies and definition-like macro calls.

//! Macro-generated C function recognition and range construction.

use tree_sitter::Node;

use super::{
    lexical::{c_declaration_prefix_token, c_identifier_char, data_symbol_name},
    preprocessor::{LocalFunctionMacroDefinition, local_function_macro_definition},
};
use crate::code::parser::nodes::{SyntaxRange, first_named_child_of_kind, node_text, syntax_range};

pub(super) enum MacroBodyFunctionDefinition {
    Recovered((String, &'static str, SyntaxRange)),
    Rejected,
    NotMacroBody,
}

pub(super) fn macro_body_function_definition(
    content: &str,
    node: Node<'_>,
) -> MacroBodyFunctionDefinition {
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

pub(super) fn syscall_macro_definition(
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

pub(super) fn macro_generated_function_definition(
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
    c_declaration_prefix_token(token) || uppercase_macro_token(token)
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
