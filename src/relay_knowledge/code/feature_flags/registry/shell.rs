//! Bounded lexical export state for shell configuration facts.
use super::*;
use tree_sitter::Node;
mod bindings;
mod getters;
mod guards;
mod options;
mod values;
pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let owned_tree;
    let root = if let Some(root) = input.syntax_root {
        root
    } else {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .map_err(|e| DomainError::invalid("shell", e.to_string()))?;
        owned_tree = parser
            .parse(input.content, None)
            .ok_or_else(|| DomainError::invalid("shell", "parse cancelled"))?;
        owned_tree.root_node()
    };
    let bindings = bindings::Bindings::build(root, input.content)?;
    let mut pending = vec![root];
    let mut rows = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "variable_assignment" && export_scope(node).is_some() {
            let explicit = node
                .parent()
                .map(|parent| export_mode(parent, input.content))
                .transpose()?
                .flatten()
                == Some(true)
                || {
                    let (external, _) = shell_external(
                        node,
                        node.child_by_field_name("name")
                            .map(|name| &input.content[name.byte_range()])
                            .unwrap_or(""),
                        input.content,
                        false,
                        &bindings,
                    )?;
                    external
                };
            let (enabled, uncertain) = if explicit {
                (false, false)
            } else {
                options::allexport(node, input.content)?
            };
            if explicit || enabled || uncertain {
                if let Some(mut row) = definition(input, node)? {
                    if (uncertain && !explicit) || export_scope(node) == Some(true) {
                        row.metadata.flow_incomplete = Some(
                            if export_scope(node) == Some(true) {
                                "conditional_export"
                            } else {
                                "conditional_allexport"
                            }
                            .into(),
                        );
                        row.metadata.default_value = None;
                        row.metadata.value_type = None;
                    }
                    check_fact_budget(rows.len())?;
                    rows.push(row);
                }
            }
        }
        if export_mode(node, input.content)? == Some(true) && export_scope(node).is_some() {
            let mut cursor = node.walk();
            for name in node.named_children(&mut cursor).filter(|n| {
                !matches!(n.kind(), "command_name" | "variable_assignment") && !n.is_extra()
            }) {
                if let Some(mut row) = definition(input, name)? {
                    if export_scope(node) == Some(true) {
                        row.metadata.default_value = None;
                        row.metadata.value_type = None;
                        row.metadata.flow_incomplete = Some("conditional_export".into());
                    }
                    check_fact_budget(rows.len())?;
                    rows.push(row);
                    continue;
                }
                let Some(key) = values::static_value(Some(name), input.content)? else {
                    continue;
                };
                if let Some((assignment, uncertain)) = prior_assignment(node, &key, input.content)?
                {
                    if let Some(mut row) = definition(input, assignment)? {
                        let site = metadata(input, node.start_byte());
                        row.metadata.domain = site.domain.or(row.metadata.domain);
                        row.metadata.hot_reload = site.hot_reload.or(row.metadata.hot_reload);
                        if let Some(comment) = node
                            .prev_named_sibling()
                            .filter(|previous| previous.kind() == "comment")
                        {
                            let gap = &input.content[comment.end_byte()..node.start_byte()];
                            if gap.trim().is_empty()
                                && gap.matches('\n').count() <= 1
                                && comment.byte_range().len() <= 8192
                            {
                                apply_annotation(
                                    &mut row.metadata,
                                    &input.content[comment.byte_range()],
                                );
                            }
                        }
                        if uncertain || export_scope(node) == Some(true) {
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
        if matches!(node.kind(), "simple_expansion" | "expansion") {
            let mut cursor = node.walk();
            if let Some(name) = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "variable_name")
            {
                let key = &input.content[name.byte_range()];
                // Positional/special parameters are shell inputs, not environment names.
                if !key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                    || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    let mut cursor = node.walk();
                    pending.extend(node.named_children(&mut cursor));
                    continue;
                }
                let (external, uncertain) =
                    shell_external(node, key, input.content, true, &bindings)?;
                if external {
                    check_fact_budget(rows.len())?;
                    let mut row = record(
                        input,
                        "env_var",
                        key,
                        "reads_config",
                        node.start_byte(),
                        node.end_byte(),
                    )?;
                    if let Some(operator) = node.child_by_field_name("operator").filter(|op| {
                        matches!(&input.content[op.byte_range()], ":-" | "-" | ":=" | "=")
                    }) {
                        let mut cursor = node.walk();
                        let fallback = node
                            .named_children(&mut cursor)
                            .find(|child| child.start_byte() >= operator.end_byte());
                        match values::static_value(fallback, input.content)?.filter(|value| {
                            value.len() <= 60 * 1024
                                && serde_json::to_string(value)
                                    .is_ok_and(|json| json.len() <= 60 * 1024)
                        }) {
                            Some(value) => {
                                row.metadata.value_type = Some(value_type(&value).into());
                                row.metadata.default_value = Some(value);
                            }
                            None => {
                                row.metadata.flow_incomplete =
                                    Some("dynamic_parameter_fallback".into())
                            }
                        }
                    }
                    if uncertain {
                        row.metadata.flow_incomplete = Some("conditional_reassignment".into());
                    }
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
                        usage
                            .metadata
                            .flow_incomplete
                            .clone_from(&row.metadata.flow_incomplete);
                        rows.push(usage);
                    }
                    rows.push(row);
                }
            }
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    let mut uncertain_reads = std::collections::BTreeMap::new();
    for row in &rows {
        if row.edge_kind == "reads_config"
            && row.metadata.flow_incomplete.as_deref() == Some("conditional_reassignment")
        {
            uncertain_reads
                .entry(row.source_key.clone())
                .and_modify(|end| *end = std::cmp::max(*end, row.byte_range.start))
                .or_insert(row.byte_range.start);
        }
    }
    for row in &mut rows {
        if row.edge_kind == "defines_config"
            && uncertain_reads
                .get(&row.source_key)
                .is_some_and(|end| row.byte_range.start <= *end)
        {
            row.metadata.default_value = None;
            row.metadata.value_type = None;
            row.metadata.flow_incomplete = Some("conditional_reassignment".into());
        }
    }
    getters::project(input, root, &mut rows)?;
    Ok(rows)
}

/// Whether an export can affect the parent environment, and whether execution is conditional.
fn export_scope(mut node: Node<'_>) -> Option<bool> {
    let mut conditional = false;
    for _ in 0..1024 {
        if node.next_sibling().is_some_and(|next| next.kind() == "&") {
            return None;
        }
        let Some(parent) = node.parent() else {
            return Some(conditional);
        };
        match parent.kind() {
            "program" | "compound_statement" | "declaration_command" => {}
            "list" => conditional |= parent.named_child(0) != Some(node),
            "if_statement" | "elif_clause" | "else_clause" | "while_statement"
            | "for_statement" | "do_group" | "case_statement" | "case_item" => conditional = true,
            _ => return None,
        }
        node = parent;
    }
    None
}

fn export_mode(node: Node<'_>, content: &str) -> Result<Option<bool>, DomainError> {
    if !matches!(
        node.kind(),
        "declaration_command" | "unset_command" | "command"
    ) {
        return Ok(None);
    }
    let mut cursor = node.walk();
    let mut words = node.children(&mut cursor).filter(|child| !child.is_extra());
    let Some(command) = node.child_by_field_name("name").or_else(|| words.next()) else {
        return Ok(None);
    };
    let command_end = command.end_byte();
    let Some(command) = values::static_value(Some(command), content)? else {
        return Ok(None);
    };
    if !matches!(
        command.as_str(),
        "export" | "declare" | "typeset" | "local" | "unset"
    ) {
        return Ok(None);
    }
    let mut decoded = Vec::new();
    for (index, word) in words.enumerate() {
        if index >= 1024 {
            return Err(DomainError::invalid(
                "configuration",
                "shell command option budget exceeded",
            ));
        }
        if word.start_byte() < command_end {
            continue;
        }
        if values::assignment(word, content)?.is_some() {
            break;
        }
        let Some(value) = values::static_value(Some(word), content)? else {
            return Ok(None);
        };
        if value == "--" || !value.starts_with(['-', '+']) {
            break;
        }
        decoded.push(value);
    }
    let options = decoded.iter().map(String::as_str).collect::<Vec<_>>();
    if options
        .iter()
        .any(|option| option.starts_with('-') && option.contains('f'))
    {
        return Ok(None);
    }
    if command == "unset"
        || options.iter().any(|option| {
            (option.starts_with('-') && option.contains('n'))
                || (option.starts_with('+') && option.contains('x'))
        })
    {
        return Ok(Some(false));
    }
    if options.contains(&"-p") {
        return Ok(None);
    }
    if command == "export"
        || options
            .iter()
            .any(|option| option.starts_with('-') && option.contains('x'))
    {
        return Ok(Some(true));
    }
    Ok((command == "local").then_some(false))
}
fn shell_external(
    mut node: Node<'_>,
    key: &str,
    content: &str,
    inherited_external: bool,
    bindings: &bindings::Bindings<'_>,
) -> Result<(bool, bool), DomainError> {
    let mut budget = 1024_usize;
    let mut assigned = false;
    let mut uncertain = false;
    while let Some(parent) = node.parent() {
        if budget == 0 {
            return Err(DomainError::invalid(
                "configuration",
                "shell export analysis incomplete: lexical budget exceeded",
            ));
        }
        budget -= 1;
        // Prefix assignment values see earlier prefixes; command arguments are
        // expanded before that temporary environment is established.
        if parent.kind() == "command" && node.kind() == "variable_assignment" {
            let mut previous = node.prev_named_sibling();
            while let Some(assignment) = previous {
                budget = budget.checked_sub(1).ok_or_else(|| {
                    DomainError::invalid(
                        "configuration",
                        "shell export analysis incomplete: lexical budget exceeded",
                    )
                })?;
                if assignment.kind() == "variable_assignment"
                    && assignment
                        .child_by_field_name("name")
                        .is_some_and(|name| &content[name.byte_range()] == key)
                {
                    return Ok((false, uncertain));
                }
                previous = assignment.prev_named_sibling();
            }
        }
        if parent.kind() == "for_statement"
            && parent.child_by_field_name("body") == Some(node)
            && parent
                .child_by_field_name("variable")
                .is_some_and(|variable| &content[variable.byte_range()] == key)
        {
            return Ok((false, uncertain));
        }
        if matches!(
            parent.kind(),
            "program"
                | "compound_statement"
                | "do_group"
                | "subshell"
                | "else_clause"
                | "elif_clause"
                | "case_item"
        ) || (parent.kind() == "if_statement"
            && !matches!(node.kind(), "else_clause" | "elif_clause"))
        {
            let children = bindings.children(key, parent);
            let end = children.partition_point(|child| child.start_byte() < node.start_byte());
            for &statement in children[..end].iter().rev() {
                let mut pending = vec![(statement, false)];
                while let Some((candidate, conditional)) = pending.pop() {
                    if budget == 0 {
                        return Err(DomainError::invalid(
                            "configuration",
                            "shell export analysis incomplete: lexical budget exceeded",
                        ));
                    }
                    budget -= 1;
                    if candidate
                        .next_sibling()
                        .is_some_and(|next| next.kind() == "&")
                    {
                        continue;
                    }
                    if candidate.kind() == "for_statement"
                        && candidate
                            .child_by_field_name("variable")
                            .is_some_and(|variable| &content[variable.byte_range()] == key)
                    {
                        uncertain |= !assigned;
                    }
                    if let Some(exported) = export_mode(candidate, content)? {
                        let names = command_names(candidate, key, content)?;
                        if names && conditional {
                            uncertain = true;
                            if !inherited_external {
                                return Ok((false, uncertain));
                            }
                        }
                        if names && !conditional {
                            if inherited_external
                                && exported
                                && export_scope(candidate) != Some(false)
                            {
                                let mut cursor = candidate.walk();
                                let mut assigns = false;
                                for child in candidate.named_children(&mut cursor).take(1024) {
                                    assigns |= values::assignment(child, content)?
                                        .is_some_and(|(name, _)| name == key);
                                }
                                let prior_local = !assigns
                                    && prior_assignment(candidate, key, content)?.is_some_and(
                                        |(assignment, conditional)| {
                                            !conditional && export_scope(assignment) != Some(false)
                                        },
                                    );
                                if assigns || prior_local {
                                    return Ok((false, uncertain));
                                }
                            }
                            return Ok((exported, uncertain));
                        }
                    }
                    if candidate.kind() == "variable_assignment"
                        && candidate
                            .child_by_field_name("name")
                            .is_some_and(|name| &content[name.byte_range()] == key)
                    {
                        if conditional {
                            uncertain |= !assigned;
                            continue;
                        }
                        if inherited_external && export_scope(candidate) != Some(false) {
                            return Ok((false, uncertain));
                        }
                        let (enabled, conditional_mode) = options::allexport(candidate, content)?;
                        if enabled || conditional_mode {
                            return Ok((true, uncertain || conditional_mode));
                        }
                        assigned = true;
                        continue;
                    }
                    let conditional = match candidate.kind() {
                        "compound_statement" | "declaration_command" | "list" => conditional,
                        "if_statement" | "elif_clause" | "else_clause" | "while_statement"
                        | "for_statement" | "do_group" | "case_statement" | "case_item" => true,
                        _ => continue,
                    };
                    for &child in bindings.children(key, candidate) {
                        if budget == 0 {
                            return Err(DomainError::invalid(
                                "configuration",
                                "shell export analysis incomplete: lexical budget exceeded",
                            ));
                        }
                        budget -= 1;
                        pending.push((
                            child,
                            conditional
                                || (candidate.kind() == "list"
                                    && candidate.named_child(0) != Some(child)),
                        ));
                    }
                }
            }
        }
        node = parent;
    }
    Ok((inherited_external && !assigned, uncertain))
}

fn definition(
    input: &FeatureFlagFileInput<'_>,
    node: Node<'_>,
) -> Result<Option<CodeFeatureFlagRecord>, DomainError> {
    let Some((key, value)) = values::assignment(node, input.content)? else {
        return Ok(None);
    };
    let mut row = record(
        input,
        "env_var",
        &key,
        "defines_config",
        node.start_byte(),
        node.end_byte(),
    )?;
    if let Some(value) = value {
        set_default(&mut row.metadata, value);
    }
    Ok(Some(row))
}
fn command_names(node: Node<'_>, key: &str, content: &str) -> Result<bool, DomainError> {
    let mut cursor = node.walk();
    for (index, child) in node.named_children(&mut cursor).enumerate() {
        if index >= 1024 {
            return Err(DomainError::invalid(
                "configuration",
                "shell command operand budget exceeded",
            ));
        }
        if child.kind() == "command_name"
            || child.is_extra()
            || node
                .child_by_field_name("name")
                .is_some_and(|name| child.start_byte() < name.end_byte())
        {
            continue;
        }
        if let Some((name, _)) = values::assignment(child, content)? {
            if name == key {
                return Ok(true);
            }
        } else if values::static_value(Some(child), content)?.as_deref() == Some(key) {
            return Ok(true);
        }
    }
    Ok(false)
}
fn prior_assignment<'a>(
    export: Node<'a>,
    key: &str,
    content: &str,
) -> Result<Option<(Node<'a>, bool)>, DomainError> {
    if export_mode(export, content)? == Some(true) && command_names(export, key, content)? {
        if let Some(command) = export.child_by_field_name("name") {
            let mut cursor = export.walk();
            let mut prefix = None;
            for (index, child) in export.named_children(&mut cursor).enumerate() {
                if index >= 1024 {
                    return Err(DomainError::invalid(
                        "configuration",
                        "shell command operand budget exceeded",
                    ));
                }
                if child.start_byte() >= command.start_byte() {
                    break;
                }
                if child.kind() == "variable_assignment"
                    && values::assignment(child, content)?.is_some_and(|(name, _)| name == key)
                {
                    prefix = Some(child);
                }
            }
            if let Some(prefix) = prefix {
                return Ok(Some((prefix, false)));
            }
        }
    }
    let mut scope = export;
    let mut previous = scope.prev_named_sibling();
    let mut budget = 1024_usize;
    let mut uncertain = false;
    loop {
        let Some(statement) = previous else {
            let Some(parent) = scope.parent().filter(|p| {
                matches!(
                    p.kind(),
                    "compound_statement"
                        | "list"
                        | "if_statement"
                        | "elif_clause"
                        | "else_clause"
                        | "while_statement"
                        | "for_statement"
                        | "do_group"
                        | "case_statement"
                        | "case_item"
                )
            }) else {
                break;
            };
            budget = budget.checked_sub(1).ok_or_else(|| {
                DomainError::invalid(
                    "configuration",
                    "shell prior assignment analysis incomplete: node budget exceeded",
                )
            })?;
            uncertain |= !matches!(parent.kind(), "compound_statement" | "list");
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
            if node.next_sibling().is_some_and(|next| next.kind() == "&") {
                continue;
            }
            if node.kind() == "for_statement"
                && node
                    .child_by_field_name("variable")
                    .is_some_and(|variable| &content[variable.byte_range()] == key)
            {
                uncertain = true;
            }
            if (node.kind() == "variable_assignment"
                || node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "command"))
                && values::assignment(node, content)?.is_some_and(|(name, _)| name == key)
            {
                if let Some(command) = node.parent().filter(|parent| {
                    parent.kind() == "command"
                        && parent
                            .child_by_field_name("name")
                            .is_some_and(|name| node.end_byte() <= name.start_byte())
                }) {
                    if export_mode(command, content)? != Some(true)
                        || !command_names(command, key, content)?
                    {
                        continue;
                    }
                }
                if conditional {
                    uncertain = true;
                    continue;
                }
                return Ok(Some((node, uncertain)));
            }
            if (node.kind() == "unset_command"
                || (node.kind() == "command"
                    && node
                        .child_by_field_name("name")
                        .map(|name| values::static_value(Some(name), content))
                        .transpose()?
                        .flatten()
                        .as_deref()
                        == Some("unset")))
                && export_mode(node, content)? == Some(false)
                && command_names(node, key, content)?
            {
                if conditional {
                    uncertain = true;
                    continue;
                }
                return Ok(None);
            }
            let conditional = match node.kind() {
                "compound_statement" | "declaration_command" | "list" => conditional,
                "command" if export_mode(node, content)?.is_some() => conditional,
                "if_statement" | "elif_clause" | "else_clause" | "while_statement"
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
                pending.push((
                    child,
                    conditional || (node.kind() == "list" && node.named_child(0) != Some(child)),
                ));
            }
        }
        previous = statement.prev_named_sibling();
    }
    Ok(None)
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
