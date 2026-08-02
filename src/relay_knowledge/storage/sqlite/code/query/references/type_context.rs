use super::identifier_text::normalized_identifier;

const REFERENCE_PARAMETER_TYPE_USAGE_BONUS: f64 = 0.45;
const REFERENCE_EXPORTED_PARAMETER_TYPE_USAGE_BONUS: f64 = 0.75;
const REFERENCE_MATCHING_PARAMETER_NAME_TYPE_BONUS: f64 = 0.65;
const REFERENCE_MATCHING_MULTILINE_PARAMETER_NAME_TYPE_BONUS: f64 = 0.25;
const REFERENCE_TYPE_USAGE_BONUS: f64 = 1.65;

#[derive(Clone, Copy)]
pub(super) struct ParameterTypeContext {
    exported_callable: bool,
}

pub(super) fn parameter_type_context(previous_lines: &[&str]) -> Option<ParameterTypeContext> {
    let context = previous_lines.join("\n");
    let open_paren = context.rfind('(')?;
    if context[open_paren + 1..].contains(')') {
        return None;
    }
    let head_line = context[..open_paren]
        .lines()
        .next_back()
        .unwrap_or_default();

    Some(ParameterTypeContext {
        exported_callable: line_starts_exported_callable(head_line),
    })
}

pub(super) fn type_reference_usage_bonus(
    line: &str,
    before: &str,
    name: &str,
    parameter_context: Option<ParameterTypeContext>,
) -> Option<f64> {
    let annotation_prefix = type_annotation_context_prefix(before)?;
    Some(
        REFERENCE_TYPE_USAGE_BONUS
            + parameter_type_reference_bonus(line, annotation_prefix, name, parameter_context),
    )
}

pub(super) fn type_annotation_context_prefix(before: &str) -> Option<&str> {
    let before = before.trim_end();
    if identifier_is_type_annotation(before) {
        return Some(before);
    }
    if let Some(prefix) = nested_type_assertion_prefix(before) {
        return Some(prefix);
    }
    let colon_index = before.rfind(':')?;
    let suffix = before[colon_index + 1..].trim();
    nested_type_context_suffix(suffix).then_some(&before[..=colon_index])
}

fn parameter_type_reference_bonus(
    line: &str,
    before: &str,
    name: &str,
    parameter_context: Option<ParameterTypeContext>,
) -> f64 {
    let same_line_parameter = type_annotation_is_callable_parameter(before);
    let multiline_parameter = !same_line_parameter
        && parameter_context.is_some()
        && type_annotation_has_parameter_name(before);
    if !same_line_parameter && !multiline_parameter {
        return 0.0;
    }
    let callable_bonus = if line_starts_exported_callable(line)
        || parameter_context.is_some_and(|context| context.exported_callable)
    {
        REFERENCE_EXPORTED_PARAMETER_TYPE_USAGE_BONUS
    } else {
        REFERENCE_PARAMETER_TYPE_USAGE_BONUS
    };
    callable_bonus + matching_parameter_name_type_bonus(before, name, same_line_parameter)
}

fn type_annotation_is_callable_parameter(before: &str) -> bool {
    let before = before.trim_end();
    let Some(prefix) = before.strip_suffix(':') else {
        return false;
    };
    let Some(open_paren) = prefix.rfind('(') else {
        return false;
    };
    if prefix[open_paren + 1..].contains(')') {
        return false;
    }

    prefix[open_paren + 1..]
        .split(',')
        .next_back()
        .is_some_and(parameter_segment_has_name)
}

fn type_annotation_has_parameter_name(before: &str) -> bool {
    before
        .trim_end()
        .strip_suffix(':')
        .is_some_and(parameter_segment_has_name)
}

fn parameter_segment_has_name(segment: &str) -> bool {
    parameter_segment_name(segment).is_some_and(|name| !name.is_empty())
}

fn matching_parameter_name_type_bonus(before: &str, type_name: &str, same_line: bool) -> f64 {
    let Some(parameter_name) = type_annotation_parameter_name(before, same_line) else {
        return 0.0;
    };
    if !parameter_name_matches_type(&parameter_name, type_name) {
        return 0.0;
    }
    if same_line {
        REFERENCE_MATCHING_PARAMETER_NAME_TYPE_BONUS
    } else {
        REFERENCE_MATCHING_MULTILINE_PARAMETER_NAME_TYPE_BONUS
    }
}

fn type_annotation_parameter_name(before: &str, same_line: bool) -> Option<String> {
    let prefix = before.trim_end().strip_suffix(':')?;
    let segment = if same_line {
        let open_paren = prefix.rfind('(')?;
        prefix[open_paren + 1..].split(',').next_back()?
    } else {
        prefix
    };
    parameter_segment_name(segment)
}

fn parameter_segment_name(segment: &str) -> Option<String> {
    let segment = segment
        .trim()
        .trim_start_matches("readonly ")
        .trim_start_matches("public ")
        .trim_start_matches("private ")
        .trim_start_matches("protected ");
    let name = segment
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())?;
    name.chars()
        .next()
        .filter(|character| *character == '_' || character.is_ascii_alphabetic())?;
    Some(name.to_owned())
}

fn parameter_name_matches_type(parameter_name: &str, type_name: &str) -> bool {
    let parameter = normalized_identifier(parameter_name);
    parameter.len() >= 4 && normalized_identifier(type_name).contains(&parameter)
}

fn line_starts_exported_callable(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("export function ")
        || line.starts_with("export async function ")
        || (line.starts_with("export const ") && (line.contains("=>") || line.contains("function")))
}

fn identifier_is_type_annotation(before: &str) -> bool {
    let before = before.trim_end();
    before.ends_with(':') || before.ends_with(" as")
}

fn nested_type_assertion_prefix(before: &str) -> Option<&str> {
    let assertion_index = before.rfind(" as ")?;
    let suffix = before[assertion_index + " as ".len()..].trim();
    nested_type_context_suffix(suffix).then_some(&before[..assertion_index + " as".len()])
}

fn nested_type_context_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix
            .chars()
            .any(|character| matches!(character, '[' | '<' | '|' | ','))
}

#[cfg(test)]
#[path = "type_context_tests.rs"]
mod tests;
