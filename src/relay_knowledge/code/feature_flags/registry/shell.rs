//! Bounded lexical export state for shell configuration facts.
use super::*;
use tree_sitter::Node;
mod guards;
mod options;
mod values;
pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
    dotenv: bool,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .map_err(|e| DomainError::invalid("shell", e.to_string()))?;
    let tree = parser
        .parse(input.content, None)
        .ok_or_else(|| DomainError::invalid("shell", "parse cancelled"))?;
    let mut pending = vec![tree.root_node()];
    let mut rows = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "variable_assignment"
            && unconditional(node)
            && (dotenv
                || node
                    .parent()
                    .and_then(|parent| export_mode(parent, input.content))
                    == Some(true)
                || options::allexport(node, input.content)?
                || shell_external(
                    node,
                    node.child_by_field_name("name")
                        .map(|name| &input.content[name.byte_range()])
                        .unwrap_or(""),
                    input.content,
                    false,
                )?)
        {
            if let Some(row) = definition(input, node)? {
                check_fact_budget(rows.len())?;
                rows.push(row);
            }
        }
        if !dotenv && export_mode(node, input.content) == Some(true) && unconditional(node) {
            let mut cursor = node.walk();
            for name in node
                .named_children(&mut cursor)
                .filter(|n| matches!(n.kind(), "word" | "variable_name"))
            {
                let key = &input.content[name.byte_range()];
                if let Some((assignment, uncertain)) = prior_assignment(node, key, input.content)? {
                    if let Some(mut row) = definition(input, assignment)? {
                        if uncertain {
                            row.metadata.default_value = None;
                            row.metadata.value_type = None;
                            row.metadata.flow_incomplete = Some("conditional_reassignment".into());
                        }
                        check_fact_budget(rows.len())?;
                        rows.push(row);
                    }
                }
            }
        }
        if !dotenv && matches!(node.kind(), "simple_expansion" | "expansion") {
            let mut cursor = node.walk();
            if let Some(name) = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "variable_name")
            {
                let key = &input.content[name.byte_range()];
                if shell_external(node, key, input.content, true)? {
                    check_fact_budget(rows.len())?;
                    let row = record(
                        input,
                        "env_var",
                        key,
                        "reads_config",
                        node.start_byte(),
                        node.end_byte(),
                    )?;
                    for guard in guards::sites(node)? {
                        check_fact_budget(rows.len() + 1)?;
                        let mut usage = record(
                            input,
                            "env_var",
                            key,
                            "guards_code",
                            guard.start_byte(),
                            guard.end_byte(),
                        )?;
                        usage.metadata.read_usage_id = Some(row.usage_id.clone());
                        rows.push(usage);
                    }
                    rows.push(row);
                }
            }
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    Ok(rows)
}

fn unconditional(mut node: Node<'_>) -> bool {
    for _ in 0..1024 {
        let Some(parent) = node.parent() else {
            return true;
        };
        if !matches!(
            parent.kind(),
            "program" | "compound_statement" | "declaration_command"
        ) {
            return false;
        }
        node = parent;
    }
    false
}

fn export_mode(node: Node<'_>, content: &str) -> Option<bool> {
    if !matches!(node.kind(), "declaration_command" | "unset_command") {
        return None;
    }
    let mut words = content[node.byte_range()].split_whitespace();
    let command = words.next()?;
    let options = words
        .take_while(|word| *word != "--" && word.starts_with(['-', '+']))
        .collect::<Vec<_>>();
    if options
        .iter()
        .any(|option| option.starts_with('-') && option.contains('f'))
    {
        return None;
    }
    if command == "unset"
        || options.iter().any(|option| {
            (option.starts_with('-') && option.contains('n'))
                || (option.starts_with('+') && option.contains('x'))
        })
    {
        return Some(false);
    }
    if options.contains(&"-p") {
        return None;
    }
    if command == "export"
        || options
            .iter()
            .any(|option| option.starts_with('-') && option.contains('x'))
    {
        return Some(true);
    }
    (command == "local").then_some(false)
}
fn shell_external(
    mut node: Node<'_>,
    key: &str,
    content: &str,
    inherited_external: bool,
) -> Result<bool, DomainError> {
    let mut budget = 1024_usize;
    let mut assigned = false;
    while let Some(parent) = node.parent() {
        if budget == 0 {
            return Err(DomainError::invalid(
                "configuration",
                "shell export analysis incomplete: lexical budget exceeded",
            ));
        }
        budget -= 1;
        if matches!(parent.kind(), "program" | "compound_statement" | "do_group") {
            let mut previous = node.prev_named_sibling();
            while let Some(statement) = previous {
                let mut pending = vec![(statement, false)];
                while let Some((candidate, conditional)) = pending.pop() {
                    if budget == 0 {
                        return Err(DomainError::invalid(
                            "configuration",
                            "shell export analysis incomplete: lexical budget exceeded",
                        ));
                    }
                    budget -= 1;
                    if let Some(exported) = export_mode(candidate, content) {
                        let mut cursor = candidate.walk();
                        let names = candidate.named_children(&mut cursor).any(|child| {
                            (matches!(child.kind(), "word" | "variable_name")
                                && &content[child.byte_range()] == key)
                                || (child.kind() == "variable_assignment"
                                    && child
                                        .child_by_field_name("name")
                                        .is_some_and(|name| &content[name.byte_range()] == key))
                        });
                        if names && conditional && !inherited_external {
                            return Ok(false);
                        }
                        if names && !conditional {
                            return Ok(exported);
                        }
                    }
                    if candidate.kind() == "variable_assignment"
                        && candidate
                            .child_by_field_name("name")
                            .is_some_and(|name| &content[name.byte_range()] == key)
                    {
                        if conditional {
                            continue;
                        }
                        if options::allexport(candidate, content)? {
                            return Ok(true);
                        }
                        assigned = true;
                        continue;
                    }
                    let conditional = match candidate.kind() {
                        "compound_statement" | "declaration_command" => conditional,
                        "list" | "if_statement" | "elif_clause" | "else_clause"
                        | "while_statement" | "for_statement" | "do_group" | "case_statement"
                        | "case_item" => true,
                        _ => continue,
                    };
                    let mut cursor = candidate.walk();
                    for child in candidate.named_children(&mut cursor) {
                        if budget == 0 {
                            return Err(DomainError::invalid(
                                "configuration",
                                "shell export analysis incomplete: lexical budget exceeded",
                            ));
                        }
                        budget -= 1;
                        pending.push((child, conditional));
                    }
                }
                previous = statement.prev_named_sibling();
            }
        }
        node = parent;
    }
    Ok(inherited_external && !assigned)
}

fn definition(
    input: &FeatureFlagFileInput<'_>,
    node: Node<'_>,
) -> Result<Option<CodeFeatureFlagRecord>, DomainError> {
    let Some(name) = node.child_by_field_name("name") else {
        return Ok(None);
    };
    let mut row = record(
        input,
        "env_var",
        &input.content[name.byte_range()],
        "defines_config",
        node.start_byte(),
        node.end_byte(),
    )?;
    let append = node
        .child_by_field_name("value")
        .is_some_and(|value| input.content[name.end_byte()..value.start_byte()].contains("+="));
    if let Some(value) = if append {
        None
    } else {
        values::static_value(node.child_by_field_name("value"), input.content)?
    } {
        row.metadata.value_type = Some(value_type(&value).to_owned());
        row.metadata.default_value = Some(value);
    }
    Ok(Some(row))
}
fn prior_assignment<'a>(
    export: Node<'a>,
    key: &str,
    content: &str,
) -> Result<Option<(Node<'a>, bool)>, DomainError> {
    let mut scope = export;
    let mut previous = scope.prev_named_sibling();
    let mut budget = 1024_usize;
    let mut uncertain = false;
    loop {
        let Some(statement) = previous else {
            let Some(parent) = scope.parent().filter(|p| p.kind() == "compound_statement") else {
                break;
            };
            budget = budget.checked_sub(1).ok_or_else(|| {
                DomainError::invalid(
                    "configuration",
                    "shell prior assignment analysis incomplete: node budget exceeded",
                )
            })?;
            scope = parent;
            previous = scope.prev_named_sibling();
            continue;
        };
        let mut pending = vec![(statement, false)];
        while let Some((node, conditional)) = pending.pop() {
            budget = budget.checked_sub(1).ok_or_else(|| {
                DomainError::invalid(
                    "configuration",
                    "shell prior assignment analysis incomplete: node budget exceeded",
                )
            })?;
            if node.kind() == "variable_assignment"
                && node
                    .child_by_field_name("name")
                    .is_some_and(|n| &content[n.byte_range()] == key)
            {
                if conditional {
                    uncertain = true;
                    continue;
                }
                return Ok(Some((node, uncertain)));
            }
            if node.kind() == "unset_command"
                && content[node.byte_range()]
                    .split_whitespace()
                    .any(|word| word == key)
            {
                return Ok(None);
            }
            let conditional = match node.kind() {
                "compound_statement" | "declaration_command" => conditional,
                "list" | "if_statement" | "elif_clause" | "else_clause" | "while_statement"
                | "for_statement" | "do_group" | "case_statement" | "case_item" => true,
                _ => continue,
            };
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                budget = budget.checked_sub(1).ok_or_else(|| {
                    DomainError::invalid(
                        "configuration",
                        "shell prior assignment analysis incomplete: node budget exceeded",
                    )
                })?;
                pending.push((child, conditional));
            }
        }
        previous = statement.prev_named_sibling();
    }
    Ok(None)
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
