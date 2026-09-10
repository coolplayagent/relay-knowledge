//! Recognize overload declarations only through a visible typing import binding.
use std::collections::BTreeMap;
use tree_sitter::Node;

use crate::code::parser::nodes::{SyntaxRange, node_text, syntax_range};
use crate::code::python_imports::{PythonModuleOrigin, PythonModuleOrigins};

const MAX_BINDING_STATEMENTS: usize = 1024;

mod bindings;
mod expressions;
use bindings::{contains_identifier, statement_binding};
mod aliases;
mod class_creation;
mod direct_execution;
mod implicit_protocols;
mod imported_modules;
mod local_classes;
mod local_instances;
mod mutation_scopes;
mod mutations;
mod pattern_bindings;
mod protocol_contract;
mod proven_aliases;
mod transparent;

struct Proof {
    origins: PythonModuleOrigins,
    decorators: BTreeMap<usize, bool>,
}

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    if node.kind() != "function_definition" || !is_overload_declaration(content, node) {
        return Vec::new();
    }
    node.child_by_field_name("name")
        .map(|name| {
            vec![(
                node_text(content, name),
                "function_declaration",
                syntax_range(node),
            )]
        })
        .unwrap_or_default()
}

pub(in crate::code::parser) fn is_overload_declaration(content: &str, function: Node<'_>) -> bool {
    is_overload_declaration_with_origins(
        content,
        function,
        PythonModuleOrigins {
            typing: PythonModuleOrigin::StandardCandidate,
            typing_extensions: PythonModuleOrigin::StandardCandidate,
        },
    )
}

/// Indexed files additionally require authorized inventory evidence for imports.
pub(in crate::code::parser) fn is_overload_declaration_with_origins(
    content: &str,
    function: Node<'_>,
    origins: PythonModuleOrigins,
) -> bool {
    let Some(decorated) = function
        .parent()
        .filter(|node| node.kind() == "decorated_definition")
    else {
        return false;
    };
    let mut remaining = MAX_BINDING_STATEMENTS;
    let mut proven = Proof {
        origins,
        decorators: BTreeMap::new(),
    };
    let mut cursor = decorated.walk();
    decorated
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "decorator")
        .any(|decorator| {
            decorator_proven(content, decorated, decorator, &mut remaining, &mut proven)
        })
}

fn evaluate_decorator(
    content: &str,
    decorated: Node<'_>,
    decorator: Node<'_>,
    remaining: &mut usize,
    proven: &mut Proof,
) -> bool {
    let Some(expression) = decorator
        .named_child(0)
        .and_then(|node| expressions::transparent(node, remaining))
    else {
        return false;
    };
    let (binding, module) = match expression.kind() {
        "identifier" => (node_text(content, expression), false),
        "attribute"
            if expression
                .child_by_field_name("attribute")
                .is_some_and(|name| node_text(content, name) == "overload") =>
        {
            let Some(object) = expression
                .child_by_field_name("object")
                .and_then(|node| expressions::transparent(node, remaining))
                .filter(|node| node.kind() == "identifier")
            else {
                return false;
            };
            (node_text(content, object), true)
        }
        _ => return false,
    };
    // Decorator expressions are evaluated in source order, before any decorator
    // is applied. Later expressions cannot change an already captured provider.
    let mut earlier = decorator.prev_named_sibling();
    while let Some(previous) = earlier {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        earlier = previous.prev_named_sibling();
        if previous.kind() == "comment" {
            continue;
        }
        if previous.kind() != "decorator" {
            return false;
        }
        let Some(expression) = previous.named_child(0) else {
            return false;
        };
        if mutations::expression_rebinds_with_budget(
            content, expression, &binding, module, remaining,
        ) || mutations::unknown_eager_call(
            content,
            expression,
            &binding,
            module,
            &proven.decorators,
            remaining,
            proven.origins,
        ) {
            return false;
        }
    }
    visible_import(content, decorated, &binding, module, remaining, proven)
}

fn prove_eager_decorators(
    content: &str,
    statement: Node<'_>,
    remaining: &mut usize,
    proven: &mut Proof,
) {
    let mut stack = vec![statement];
    while let Some(node) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return;
        };
        *remaining = left;
        match node.kind() {
            "decorator" => {
                if let Some(owner) = node.parent() {
                    decorator_proven(content, owner, node, remaining, proven);
                }
                continue;
            }
            "class_definition" => {
                if let Some(body) = node.child_by_field_name("body") {
                    let Some(left) = remaining.checked_sub(1) else {
                        return;
                    };
                    *remaining = left;
                    stack.push(body);
                }
                continue;
            }
            "decorated_definition"
            | "block"
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "with_statement"
            | "try_statement"
            | "except_clause"
            | "else_clause"
            | "finally_clause"
            | "match_statement"
            | "case_clause" => {}
            // Expressions cannot contain a statement decorator. Undecorated
            // function bodies run later, so neither needs a recursive proof.
            _ => continue,
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let Some(left) = remaining.checked_sub(1) else {
                return;
            };
            *remaining = left;
            stack.push(child);
        }
    }
}

fn visible_import(
    content: &str,
    mut node: Node<'_>,
    binding: &str,
    module: bool,
    remaining: &mut usize,
    proven: &mut Proof,
) -> bool {
    let decorated = node;
    let mut crossed_scope = false;
    let mut delayed_lookup = false;
    'lookup: loop {
        if *remaining == 0 {
            return false;
        }
        *remaining -= 1;
        if node.kind() == "case_clause"
            && (!crossed_scope || !class_namespace(node))
            && pattern_bindings::binds(content, node, binding, remaining)
        {
            return false;
        }
        if delayed_lookup
            && node.parent().is_some_and(|parent| {
                matches!(parent.kind(), "module" | "block")
                    && (!crossed_scope || !class_namespace(parent))
            })
        {
            if let Some(imported) = later_import_binding(
                content,
                node,
                decorated,
                binding,
                module,
                remaining,
                proven.origins,
            ) {
                return imported;
            }
        }
        // Only module/block children are lexical statements. Parameter and
        // annotation siblings of a body are not preceding assignments.
        let mut previous = node
            .parent()
            .filter(|parent| matches!(parent.kind(), "module" | "block"))
            .filter(|parent| !crossed_scope || !class_namespace(*parent))
            .and_then(|_| node.prev_named_sibling());
        while let Some(statement) = previous {
            if *remaining == 0 {
                return false;
            }
            *remaining -= 1;
            if matches!(statement.kind(), "global_statement" | "nonlocal_statement")
                && contains_identifier(content, statement, binding)
            {
                let global = statement.kind() == "global_statement";
                if global
                    && statement
                        .parent()
                        .is_some_and(|parent| parent.kind() == "module")
                {
                    previous = statement.prev_named_sibling();
                    continue;
                }
                // Directives redirect lookup; unlike assignment/deletion they
                // do not establish a new value for the decorator binding.
                while let Some(parent) = node.parent() {
                    if *remaining == 0 {
                        return false;
                    }
                    *remaining -= 1;
                    if global && parent.kind() == "module" {
                        break;
                    }
                    node = parent;
                    if !global && matches!(node.kind(), "function_definition" | "class_definition")
                    {
                        break;
                    }
                }
                crossed_scope = true;
                delayed_lookup = true;
                continue 'lookup;
            }
            if let Some(imported) =
                statement_binding(content, statement, binding, module, proven.origins)
            {
                return imported;
            }
            prove_eager_decorators(content, statement, remaining, proven);
            if mutations::unknown_eager_call(
                content,
                statement,
                binding,
                module,
                &proven.decorators,
                remaining,
                proven.origins,
            ) {
                return false;
            }
            previous = statement.prev_named_sibling();
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        if parent.kind() == "function_definition"
            && node.kind() == "block"
            && bindings::function_local(content, node, binding, remaining)
        {
            return false;
        }
        if parent.kind() == "function_definition"
            && parent
                .child_by_field_name("parameters")
                .is_some_and(|parameters| parameter_binds(content, parameters, binding))
        {
            return false;
        }
        // A method's decorators can use their immediate class namespace, but
        // nested functions/classes do not close over an enclosing class body.
        delayed_lookup |= parent.kind() == "function_definition";
        crossed_scope |= matches!(parent.kind(), "function_definition" | "class_definition");
        node = parent;
    }
}

fn later_import_binding(
    content: &str,
    node: Node<'_>,
    decorated: Node<'_>,
    binding: &str,
    module: bool,
    remaining: &mut usize,
    origins: PythonModuleOrigins,
) -> Option<bool> {
    let mut next = node.next_named_sibling();
    let mut proven = None;
    let mut execution_boundary = false;
    let callable_name = (node.kind() == "function_definition"
        && direct_execution::reaches(content, node, decorated, remaining))
    .then(|| node.child_by_field_name("name"))
    .flatten()
    .map(|name| node_text(content, name));
    let mut callable_unchanged = true;
    while let Some(statement) = next {
        if *remaining == 0 {
            return Some(false);
        }
        *remaining -= 1;
        if !execution_boundary
            && callable_unchanged
            && callable_name
                .as_deref()
                .is_some_and(|name| direct_function_call(content, statement, name, remaining))
        {
            // Later calls, including calls through aliases, can evaluate the
            // same decorator again after a provider write. Only a nonexecuting
            // linear tail makes this invocation sufficient evidence.
            let mut later = statement.next_named_sibling();
            while let Some(tail) = later {
                let Some(left) = remaining.checked_sub(1) else {
                    return Some(false);
                };
                *remaining = left;
                if !matches!(
                    tail.kind(),
                    "expression_statement" | "pass_statement" | "comment"
                ) || !linear_binding_statement(tail)
                    || expressions::has_eager_call(content, tail, remaining)
                {
                    return Some(false);
                }
                later = tail.next_named_sibling();
            }
            return proven;
        }
        if expressions::has_eager_call(content, statement, remaining) {
            return Some(false);
        }
        if callable_name.as_deref().is_some_and(|name| {
            statement_binding(content, statement, name, false, origins).is_some()
        }) {
            callable_unchanged = false;
        }
        if !matches!(statement.kind(), "global_statement" | "nonlocal_statement") {
            let linear = linear_binding_statement(statement);
            if let Some(value) = statement_binding(content, statement, binding, module, origins) {
                if execution_boundary || !linear {
                    return Some(false);
                }
                // A later proven import can replace a prior value, but a still
                // later write must win. Never cross a call/return/control path.
                proven = Some(value);
            }
            execution_boundary |= !linear;
        }
        next = statement.next_named_sibling();
    }
    proven
}

fn direct_function_call(
    content: &str,
    statement: Node<'_>,
    name: &str,
    remaining: &mut usize,
) -> bool {
    if !matches!(
        statement.kind(),
        "expression_statement" | "return_statement"
    ) {
        return false;
    }
    let Some(mut expression) = statement.named_child(0) else {
        return false;
    };
    if expression.kind() == "assignment" {
        let Some(value) = expression.child_by_field_name("right") else {
            return false;
        };
        expression = value;
    }
    let Some(call) =
        expressions::transparent(expression, remaining).filter(|node| node.kind() == "call")
    else {
        return false;
    };
    let Some(function) = call
        .child_by_field_name("function")
        .and_then(|node| expressions::transparent(node, remaining))
        .filter(|node| node.kind() == "identifier")
    else {
        return false;
    };
    node_text(content, function) == name
        && call
            .child_by_field_name("arguments")
            .is_some_and(|arguments| arguments.named_child_count() == 0)
}

fn linear_binding_statement(statement: Node<'_>) -> bool {
    match statement.kind() {
        "import_statement" | "import_from_statement" | "pass_statement" | "comment" => true,
        "expression_statement" => statement
            .named_child(0)
            .filter(|expression| expression.kind() == "assignment")
            .and_then(|expression| expression.child_by_field_name("right"))
            .is_some_and(|value| {
                matches!(
                    value.kind(),
                    "identifier"
                        | "lambda"
                        | "string"
                        | "integer"
                        | "float"
                        | "true"
                        | "false"
                        | "none"
                )
            }),
        _ => false,
    }
}

fn class_namespace(mut node: Node<'_>) -> bool {
    for _ in 0..MAX_BINDING_STATEMENTS {
        match node.kind() {
            "class_definition" => return true,
            "function_definition" | "module" => return false,
            _ => {}
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
    // An unproven namespace must not supply a typing import binding.
    true
}

fn parameter_binds(content: &str, parameters: Node<'_>, binding: &str) -> bool {
    let mut cursor = parameters.walk();
    parameters.named_children(&mut cursor).any(|parameter| {
        let name = match parameter.kind() {
            "identifier" => Some(parameter),
            "default_parameter" | "typed_default_parameter" => {
                parameter.child_by_field_name("name")
            }
            "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                parameter.named_child(0)
            }
            _ => None,
        };
        name.is_some_and(|name| contains_identifier(content, name, binding))
    })
}

/// Reuse already-proven earlier decorators within this one bounded lookup.
/// Each new cache entry consumed the shared budget, so recursive alias proofs
/// cannot multiply work or allocate beyond the statement budget.
fn decorator_proven(
    content: &str,
    decorated: Node<'_>,
    decorator: Node<'_>,
    remaining: &mut usize,
    proven: &mut Proof,
) -> bool {
    let Some(left) = remaining.checked_sub(1) else {
        return false;
    };
    *remaining = left;
    if let Some(value) = proven.decorators.get(&decorator.start_byte()) {
        return *value;
    }
    let value = evaluate_decorator(content, decorated, decorator, remaining, proven);
    proven.decorators.insert(decorator.start_byte(), value);
    value
}

#[cfg(test)]
#[path = "overloads/execution_boundary_tests.rs"]
mod execution_boundary_tests;
