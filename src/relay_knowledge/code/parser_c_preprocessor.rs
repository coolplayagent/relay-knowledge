use std::collections::HashSet;

pub(super) struct MacroFunctionDefinition {
    pub(super) parameters: Vec<String>,
    pub(super) replacement: String,
}

pub(super) enum LocalFunctionMacroDefinition {
    Function(MacroFunctionDefinition),
    ActiveNonFunction,
    Unavailable,
    Missing,
}

pub(super) fn local_function_macro_definition(
    content: &str,
    macro_name: &str,
    limit_byte: usize,
) -> LocalFunctionMacroDefinition {
    let search_end = limit_byte.min(content.len());
    let mut active_macros = HashSet::new();
    let mut branches = Vec::new();
    let mut latest = None;
    let mut active_non_function = false;
    let mut seen_unavailable_macro = false;
    let mut logical_line = String::new();
    for line in content[..search_end].lines() {
        if !logical_line.is_empty() {
            append_preprocessor_logical_line(&mut logical_line, line);
            if !line_continues_preprocessor_definition(line) {
                apply_define_logical_line(
                    &logical_line,
                    macro_name,
                    &mut active_macros,
                    &mut latest,
                    &mut active_non_function,
                );
                logical_line.clear();
            }
            continue;
        }
        let trimmed_start = line.trim_start();
        let Some(directive) = preprocessor_directive(trimmed_start) else {
            continue;
        };
        if update_preprocessor_branch(&directive, &active_macros, &mut branches) {
            continue;
        }
        if !preprocessor_branches_active(&branches) {
            seen_unavailable_macro |= directive.keyword == "define"
                && directive_identifier(directive.rest).is_some_and(|name| name == macro_name);
            continue;
        }
        match directive.keyword {
            "define" => {
                append_preprocessor_logical_line(&mut logical_line, line);
                if !line_continues_preprocessor_definition(line) {
                    apply_define_logical_line(
                        &logical_line,
                        macro_name,
                        &mut active_macros,
                        &mut latest,
                        &mut active_non_function,
                    );
                    logical_line.clear();
                }
            }
            "undef" => {
                if let Some(name) = directive_identifier(directive.rest) {
                    active_macros.remove(name);
                    if name == macro_name {
                        seen_unavailable_macro = true;
                        active_non_function = false;
                        latest = None;
                    }
                }
            }
            _ => {}
        }
    }
    if !logical_line.is_empty() {
        apply_define_logical_line(
            &logical_line,
            macro_name,
            &mut active_macros,
            &mut latest,
            &mut active_non_function,
        );
    }

    match (latest, active_non_function, seen_unavailable_macro) {
        (Some(definition), _, _) => LocalFunctionMacroDefinition::Function(definition),
        (None, true, _) => LocalFunctionMacroDefinition::ActiveNonFunction,
        (None, false, true) => LocalFunctionMacroDefinition::Unavailable,
        (None, false, false) => LocalFunctionMacroDefinition::Missing,
    }
}

struct PreprocessorDirective<'a> {
    keyword: &'a str,
    rest: &'a str,
}

fn preprocessor_directive(line: &str) -> Option<PreprocessorDirective<'_>> {
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

fn apply_define_logical_line(
    line: &str,
    macro_name: &str,
    active_macros: &mut HashSet<String>,
    latest: &mut Option<MacroFunctionDefinition>,
    active_non_function: &mut bool,
) {
    let Some(directive) = preprocessor_directive(line.trim_start()) else {
        return;
    };
    if directive.keyword != "define" {
        return;
    }
    let Some(name) = directive_identifier(directive.rest) else {
        return;
    };
    active_macros.insert(name.to_owned());
    if name == macro_name {
        *latest = parse_function_macro_definition_line(line, macro_name);
        *active_non_function = latest.is_none();
    }
}

struct PreprocessorBranch {
    parent_active: bool,
    branch_active: bool,
    branch_taken: bool,
}

fn update_preprocessor_branch(
    directive: &PreprocessorDirective<'_>,
    active_macros: &HashSet<String>,
    branches: &mut Vec<PreprocessorBranch>,
) -> bool {
    match directive.keyword {
        "if" => {
            push_preprocessor_branch(
                branches,
                evaluate_if_condition(directive.rest, active_macros),
            );
            true
        }
        "ifdef" => {
            let active = directive_identifier(directive.rest)
                .is_some_and(|name| active_macros.contains(name));
            push_preprocessor_branch(branches, active);
            true
        }
        "ifndef" => {
            let active = directive_identifier(directive.rest)
                .is_none_or(|name| !active_macros.contains(name));
            push_preprocessor_branch(branches, active);
            true
        }
        "elif" => {
            let active = evaluate_if_condition(directive.rest, active_macros);
            apply_preprocessor_elif(branches, active);
            true
        }
        "else" => {
            apply_preprocessor_else(branches);
            true
        }
        "endif" => {
            branches.pop();
            true
        }
        _ => false,
    }
}

fn push_preprocessor_branch(branches: &mut Vec<PreprocessorBranch>, condition_active: bool) {
    let parent_active = preprocessor_branches_active(branches);
    let branch_active = parent_active && condition_active;
    branches.push(PreprocessorBranch {
        parent_active,
        branch_active,
        branch_taken: branch_active,
    });
}

fn apply_preprocessor_elif(branches: &mut [PreprocessorBranch], condition_active: bool) {
    let Some(branch) = branches.last_mut() else {
        return;
    };
    branch.branch_active = branch.parent_active && !branch.branch_taken && condition_active;
    branch.branch_taken |= branch.branch_active;
}

fn apply_preprocessor_else(branches: &mut [PreprocessorBranch]) {
    let Some(branch) = branches.last_mut() else {
        return;
    };
    branch.branch_active = branch.parent_active && !branch.branch_taken;
    branch.branch_taken = true;
}

fn preprocessor_branches_active(branches: &[PreprocessorBranch]) -> bool {
    branches.last().is_none_or(|branch| branch.branch_active)
}

fn evaluate_if_condition(expression: &str, active_macros: &HashSet<String>) -> bool {
    let expression = expression.trim();
    if let Ok(value) = expression.parse::<i64>() {
        return value != 0;
    }
    if let Some(name) = defined_expression_name(expression) {
        return active_macros.contains(name);
    }
    if directive_identifier(expression).is_some_and(|name| name == expression) {
        return active_macros.contains(expression);
    }
    if let Some(name) = expression
        .strip_prefix('!')
        .map(str::trim_start)
        .and_then(|value| {
            defined_expression_name(value)
                .or_else(|| directive_identifier(value).filter(|name| *name == value))
        })
    {
        return !active_macros.contains(name);
    }

    false
}

fn defined_expression_name(expression: &str) -> Option<&str> {
    let rest = expression.trim().strip_prefix("defined")?.trim_start();
    if let Some(inner) = rest
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    {
        return directive_identifier(inner.trim());
    }

    directive_identifier(rest)
}

fn directive_identifier(text: &str) -> Option<&str> {
    let end = text
        .find(|character: char| !c_identifier_char(character))
        .unwrap_or(text.len());
    let name = &text[..end];
    if name.is_empty() {
        return None;
    }

    Some(name)
}

fn append_preprocessor_logical_line(logical_line: &mut String, line: &str) {
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

fn line_continues_preprocessor_definition(line: &str) -> bool {
    line.trim_end().ends_with('\\')
}

fn parse_function_macro_definition_line(
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

fn c_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_macro_lookup_accepts_spaced_define_directives() {
        let content = "# define KONG_ACCESS_PHASE(name) \\\n    static ngx_int_t name(ngx_http_request_t *request)\n";
        let LocalFunctionMacroDefinition::Function(definition) =
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
        else {
            panic!("spaced define directive should be visible");
        };

        assert_eq!(definition.parameters, ["name"]);
        assert!(definition.replacement.contains("name("));
    }

    #[test]
    fn local_macro_lookup_respects_undef() {
        let content = "\
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#undef KONG_ACCESS_PHASE
";

        assert!(matches!(
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len()),
            LocalFunctionMacroDefinition::Unavailable
        ));
    }

    #[test]
    fn local_macro_lookup_ignores_inactive_branches() {
        let content = "\
#if 0
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
#if FEATURE_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
#ifdef NEVER_DEFINED
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";

        assert!(matches!(
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len()),
            LocalFunctionMacroDefinition::Unavailable
        ));
    }
}
