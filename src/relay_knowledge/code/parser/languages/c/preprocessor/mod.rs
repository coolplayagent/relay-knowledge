//! Active C preprocessor branch and macro lifecycle evaluation.

use std::collections::HashMap;

use conditions::evaluate_if_condition;
use directives::{
    PreprocessorDirective, append_preprocessor_logical_line, directive_identifier,
    line_continues_preprocessor_directive, parse_active_macro_definition_line,
    parse_function_macro_definition_line, preprocessor_directive,
};

mod conditions;
mod directives;

struct ActiveMacroDefinition {
    replacement: String,
    function_like: bool,
}

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
    let mut active_macros = HashMap::new();
    let mut branches = Vec::new();
    let mut latest = None;
    let mut active_non_function = false;
    let mut seen_unavailable_macro = false;
    let mut logical_line = String::new();
    for line in content[..search_end].lines() {
        if !logical_line.is_empty() {
            append_preprocessor_logical_line(&mut logical_line, line);
            if !line_continues_preprocessor_directive(line) {
                apply_preprocessor_logical_line(
                    &logical_line,
                    macro_name,
                    &mut active_macros,
                    &mut branches,
                    &mut latest,
                    &mut active_non_function,
                    &mut seen_unavailable_macro,
                );
                logical_line.clear();
            }
            continue;
        }
        let trimmed_start = line.trim_start();
        if preprocessor_directive(trimmed_start).is_none() {
            continue;
        }
        append_preprocessor_logical_line(&mut logical_line, line);
        if !line_continues_preprocessor_directive(line) {
            apply_preprocessor_logical_line(
                &logical_line,
                macro_name,
                &mut active_macros,
                &mut branches,
                &mut latest,
                &mut active_non_function,
                &mut seen_unavailable_macro,
            );
            logical_line.clear();
        }
    }
    if !logical_line.is_empty() {
        apply_preprocessor_logical_line(
            &logical_line,
            macro_name,
            &mut active_macros,
            &mut branches,
            &mut latest,
            &mut active_non_function,
            &mut seen_unavailable_macro,
        );
    }

    match (latest, active_non_function, seen_unavailable_macro) {
        (Some(definition), _, _) => LocalFunctionMacroDefinition::Function(definition),
        (None, true, _) => LocalFunctionMacroDefinition::ActiveNonFunction,
        (None, false, true) => LocalFunctionMacroDefinition::Unavailable,
        (None, false, false) => LocalFunctionMacroDefinition::Missing,
    }
}

fn apply_preprocessor_logical_line(
    line: &str,
    macro_name: &str,
    active_macros: &mut HashMap<String, ActiveMacroDefinition>,
    branches: &mut Vec<PreprocessorBranch>,
    latest: &mut Option<MacroFunctionDefinition>,
    active_non_function: &mut bool,
    seen_unavailable_macro: &mut bool,
) {
    let Some(directive) = preprocessor_directive(line.trim_start()) else {
        return;
    };
    if update_preprocessor_branch(&directive, active_macros, branches) {
        return;
    }
    if !preprocessor_branches_active(branches) {
        *seen_unavailable_macro |= directive.keyword == "define"
            && directive_identifier(directive.rest).is_some_and(|name| name == macro_name);
        return;
    }

    match directive.keyword {
        "define" => {
            apply_define_logical_line(line, macro_name, active_macros, latest, active_non_function);
        }
        "undef" => {
            if let Some(name) = directive_identifier(directive.rest) {
                active_macros.remove(name);
                if name == macro_name {
                    *seen_unavailable_macro = true;
                    *active_non_function = false;
                    *latest = None;
                }
            }
        }
        _ => {}
    }
}

fn apply_define_logical_line(
    line: &str,
    macro_name: &str,
    active_macros: &mut HashMap<String, ActiveMacroDefinition>,
    latest: &mut Option<MacroFunctionDefinition>,
    active_non_function: &mut bool,
) {
    let Some((name, active_macro)) = parse_active_macro_definition_line(line) else {
        return;
    };
    active_macros.insert(name.clone(), active_macro);
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
    active_macros: &HashMap<String, ActiveMacroDefinition>,
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
                .is_some_and(|name| active_macros.contains_key(name));
            push_preprocessor_branch(branches, active);
            true
        }
        "ifndef" => {
            let active = directive_identifier(directive.rest)
                .is_none_or(|name| !active_macros.contains_key(name));
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
