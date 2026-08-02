use super::super::lexical::c_identifier_char;
use super::{ActiveMacroDefinition, MacroFunctionDefinition};

pub(super) struct PreprocessorDirective<'a> {
    pub(super) keyword: &'a str,
    pub(super) rest: &'a str,
}

pub(super) fn preprocessor_directive(line: &str) -> Option<PreprocessorDirective<'_>> {
    let directive = line.strip_prefix('#')?.trim_start();
    let keyword_end = directive
        .find(|character: char| !c_identifier_char(character))
        .unwrap_or(directive.len());
    let keyword = &directive[..keyword_end];
    if keyword.is_empty() {
        return None;
    }

    Some(PreprocessorDirective {
        keyword,
        rest: directive[keyword_end..].trim_start(),
    })
}

pub(super) fn directive_identifier(text: &str) -> Option<&str> {
    let end = text
        .find(|character: char| !c_identifier_char(character))
        .unwrap_or(text.len());
    let name = &text[..end];
    if name.is_empty() {
        return None;
    }

    Some(name)
}

pub(super) fn append_preprocessor_logical_line(logical_line: &mut String, line: &str) {
    if !logical_line.is_empty() {
        logical_line.push(' ');
    }
    let segment = line
        .trim_end()
        .strip_suffix('\\')
        .unwrap_or_else(|| line.trim_end())
        .trim();
    logical_line.push_str(segment);
}

pub(super) fn line_continues_preprocessor_directive(line: &str) -> bool {
    line.trim_end().ends_with('\\')
}

pub(super) fn parse_active_macro_definition_line(
    line: &str,
) -> Option<(String, ActiveMacroDefinition)> {
    let directive = preprocessor_directive(line.trim_start())?;
    if directive.keyword != "define" {
        return None;
    }
    let name = directive_identifier(directive.rest)?;
    let after_name = &directive.rest[name.len()..];
    let function_like = after_name.starts_with('(');
    let replacement = if function_like {
        let parameters_end = closing_parenthesis_index(after_name)?;
        after_name[parameters_end + 1..].trim()
    } else {
        after_name.trim()
    };

    Some((
        name.to_owned(),
        ActiveMacroDefinition {
            replacement: replacement.to_owned(),
            function_like,
        },
    ))
}

pub(super) fn parse_function_macro_definition_line(
    line: &str,
    macro_name: &str,
) -> Option<MacroFunctionDefinition> {
    let directive = preprocessor_directive(line.trim_start())?;
    if directive.keyword != "define" {
        return None;
    }
    let after_name = directive.rest.strip_prefix(macro_name)?;
    if !after_name.starts_with('(') {
        return None;
    }
    let parameters_end = closing_parenthesis_index(after_name)?;
    let replacement = after_name[parameters_end + 1..].trim();
    if replacement.is_empty() {
        return None;
    }
    let parameters = after_name[1..parameters_end]
        .split(',')
        .map(str::trim)
        .filter(|parameter| !parameter.is_empty() && *parameter != "...")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if parameters.is_empty() {
        return None;
    }

    Some(MacroFunctionDefinition {
        parameters,
        replacement: replacement.to_owned(),
    })
}

fn closing_parenthesis_index(text: &str) -> Option<usize> {
    if !text.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
#[path = "directives_tests.rs"]
mod tests;
