//! Bounded shell output getters and command-substitution-to-condition flow.
use super::*;
use std::collections::BTreeMap;

pub(super) fn project(
    input: &FeatureFlagFileInput<'_>,
    root: Node<'_>,
    rows: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    let mut nodes = Vec::new();
    let mut cursor = root.walk();
    loop {
        if nodes.len() == 262_144 {
            return Err(DomainError::invalid(
                "configuration",
                "shell syntax budget exceeded",
            ));
        }
        nodes.push(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break;
            }
        }
        if cursor.node() == root {
            break;
        }
    }
    let mut reads_by_start = BTreeMap::<usize, Vec<usize>>::new();
    for (index, row) in rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.edge_kind == "reads_config")
    {
        reads_by_start
            .entry(row.byte_range.start as usize)
            .or_default()
            .push(index);
    }
    let mut function_counts = BTreeMap::<&str, usize>::new();
    for node in &nodes {
        if node.kind() == "function_definition"
            && let Some(name) = node.child_by_field_name("name")
        {
            *function_counts
                .entry(&input.content[name.byte_range()])
                .or_default() += 1;
        }
    }
    let mut providers = BTreeMap::<String, Vec<(String, Node<'_>)>>::new();
    let mut imports = Vec::new();
    let mut invalidations = Vec::new();
    for node in &nodes {
        if node.kind() == "unset_command" {
            invalidations.push(node.start_byte());
        }
        if node.kind() == "command" {
            if let Some(name) = node.child_by_field_name("name")
                && matches!(
                    input.content[name.byte_range()].trim(),
                    "." | "source" | "eval" | "unset" | "unalias"
                )
            {
                invalidations.push(node.start_byte());
            }
            if let Some((path, anchored)) = anchored_source(*node, input.path, input.content)
                && node.parent() == Some(root)
                && !function_counts.contains_key("dirname")
            {
                imports.push((path, *node, anchored));
            } else if let Some(words) = command_words(*node, input.content)
                && words.len() == 2
                && matches!(words[0].as_str(), "." | "source")
                && node.parent() == Some(root)
                && let Some(path) = relative_source(input.path, &words[1])
            {
                imports.push((path, *node, false));
            }
        }
        if node.kind() != "function_definition" || node.parent() != Some(root) {
            continue;
        }
        let Some(name) = node.child_by_field_name("name") else {
            continue;
        };
        let name = &input.content[name.byte_range()];
        if function_counts.get(name) != Some(&1) {
            continue;
        }
        let Some(body) = node.child_by_field_name("body") else {
            continue;
        };
        let mut cursor = body.walk();
        let commands: Vec<_> = body
            .named_children(&mut cursor)
            .filter(|n| !n.is_extra())
            .take(2)
            .collect();
        if commands.len() != 1 || commands[0].kind() != "command" {
            continue;
        }
        let command = commands[0];
        let Some(head) = command.child_by_field_name("name") else {
            continue;
        };
        let head = &input.content[head.byte_range()];
        let mut cursor = command.walk();
        let arguments: Vec<_> = command
            .named_children(&mut cursor)
            .filter(|n| *n != command.child_by_field_name("name").unwrap() && !n.is_extra())
            .take(4)
            .collect();
        let output = match (head, arguments.as_slice()) {
            ("echo", [output]) => Some(*output),
            ("printf", [format, output])
                if values::static_value(Some(*format), input.content)?.as_deref() == Some("%s") =>
            {
                Some(*output)
            }
            _ => None,
        };
        let Some(output) = output else {
            continue;
        };
        let candidates: Vec<_> = reads_by_start
            .range(output.start_byte()..output.end_byte())
            .flat_map(|(_, indices)| indices.iter().copied())
            .filter(|i| rows[*i].byte_range.end as usize <= output.end_byte())
            .take(2)
            .collect();
        if candidates.len() != 1 {
            continue;
        }
        let row = &mut rows[candidates[0]];
        let expansion = &input.content[row.byte_range.start as usize..row.byte_range.end as usize];
        let emitted = input.content[output.byte_range()].trim_matches(['\'', '"']);
        if emitted != expansion {
            continue;
        }
        let binding = format!("shell|{}||{name}", input.path);
        row.metadata.bindings.push(binding.clone());
        row.metadata.declared_getter = Some(binding.clone());
        providers
            .entry(name.to_owned())
            .or_default()
            .push((binding, *node));
    }
    // A sourced file exposes its final function bindings. A later source,
    // eval or unset can replace a provider before any importing script uses it.
    for row in rows.iter_mut() {
        if let Some(binding) = row.metadata.declared_getter.as_ref()
            && providers.values().flatten().any(|(candidate, function)| {
                candidate == binding && invalidations.iter().any(|pos| *pos > function.end_byte())
            })
        {
            row.metadata
                .bindings
                .retain(|candidate| candidate != binding);
            row.metadata.declared_getter = None;
            row.metadata.flow_incomplete = Some("shell_function_binding_invalidated".into());
        }
    }
    let mut assignments = BTreeMap::<String, Vec<(Node<'_>, CodeFeatureFlagRecord)>>::new();
    for node in &nodes {
        if node.kind() != "command_substitution" {
            continue;
        }
        let mut cursor = node.walk();
        let commands: Vec<_> = node
            .named_children(&mut cursor)
            .filter(|n| !n.is_extra())
            .take(2)
            .collect();
        if commands.len() != 1 {
            continue;
        }
        let Some(words) = command_words(commands[0], input.content).filter(|w| w.len() == 1) else {
            continue;
        };
        let name = &words[0];
        let prior_imports: Vec<_> = imports
            .iter()
            .filter(|(_, import, _)| import.end_byte() < node.start_byte())
            .take(2)
            .collect();
        let (reference, incomplete) = if let Some(bindings) = providers
            .get(name)
            .filter(|p| p.len() == 1 && p[0].1.end_byte() < node.start_byte())
        {
            if invalidations
                .iter()
                .any(|pos| *pos > bindings[0].1.end_byte() && *pos < node.start_byte())
            {
                (
                    format!("shell-getter-unresolved|{}||{name}", input.path),
                    Some("shell_function_binding_invalidated".into()),
                )
            } else {
                (bindings[0].0.clone(), None)
            }
        } else if prior_imports.len() == 1 {
            // A relative source path is evaluated against the process working
            // directory, not this script's directory. Preserve the hint without
            // joining it to a provider under an unproved directory assumption.
            if prior_imports[0].2
                && !invalidations
                    .iter()
                    .any(|pos| *pos > prior_imports[0].1.end_byte() && *pos < node.start_byte())
            {
                (format!("shell|{}||{name}", prior_imports[0].0), None)
            } else {
                (
                    format!("shell-source-unresolved|{}||{name}", prior_imports[0].0),
                    Some("shell_source_working_directory_unknown".to_owned()),
                )
            }
        } else {
            continue;
        };
        let mut row = record(
            input,
            "config_symbol",
            &reference,
            "reads_config",
            node.start_byte(),
            node.end_byte(),
        )?;
        row.metadata.reference = Some(reference);
        row.metadata.exact_reference = true;
        row.metadata.flow_incomplete = incomplete;
        let mut parent = node.parent();
        for _ in 0..16 {
            let Some(current) = parent else {
                break;
            };
            if current.kind() == "variable_assignment" {
                if let Some(name) = current.child_by_field_name("name") {
                    assignments
                        .entry(input.content[name.byte_range()].to_owned())
                        .or_default()
                        .push((current, row.clone()));
                }
                break;
            }
            if !matches!(current.kind(), "string") {
                break;
            }
            parent = current.parent();
        }
        for guard in guards::sites(*node)? {
            push_guard(input, guard, &row, rows)?;
        }
        check_fact_budget(rows.len())?;
        rows.push(row);
    }
    for node in &nodes {
        if !matches!(node.kind(), "simple_expansion" | "expansion") {
            continue;
        }
        let Some(name) = node.named_child(0) else {
            continue;
        };
        let name = &input.content[name.byte_range()];
        let Some(candidates) = assignments.get(name).filter(|p| p.len() == 1) else {
            continue;
        };
        let (assignment, read) = &candidates[0];
        for guard in guards::sites(*node)? {
            let statement = guard.parent().unwrap_or(guard);
            let Some((prior, uncertain)) = prior_assignment(statement, name, input.content)? else {
                continue;
            };
            if prior != *assignment || uncertain {
                continue;
            }
            push_guard(input, guard, read, rows)?;
        }
    }
    Ok(())
}

/// Recognize the standard script-directory source idiom as structured shell
/// syntax. Relative process-directory sources remain uncertain.
fn anchored_source(node: Node<'_>, path: &str, source: &str) -> Option<(String, bool)> {
    let name = node.child_by_field_name("name")?;
    if !matches!(source[name.byte_range()].trim(), "source" | ".") {
        return None;
    }
    let mut cursor = node.walk();
    let args = node
        .named_children(&mut cursor)
        .filter(|n| *n != name && !n.is_extra())
        .take(3)
        .collect::<Vec<_>>();
    if args.len() != 1 || args[0].kind() != "string" {
        return None;
    }
    let value = args[0];
    let mut cursor = value.walk();
    let substitutions = value
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "command_substitution")
        .take(2)
        .collect::<Vec<_>>();
    if substitutions.len() != 1 {
        return None;
    }
    let raw = source[value.byte_range()].trim();
    let suffix = raw
        .strip_prefix("\"$(dirname \"${BASH_SOURCE[0]}\")/")?
        .strip_suffix('"')?;
    if suffix.contains(['$', '`', '\\', '"']) {
        return None;
    }
    relative_source(path, suffix).map(|path| (path, true))
}

fn command_words(node: Node<'_>, source: &str) -> Option<Vec<String>> {
    if node.kind() != "command" {
        return None;
    }
    let mut cursor = node.walk();
    let mut words = Vec::new();
    for child in node.named_children(&mut cursor).filter(|n| !n.is_extra()) {
        if words.len() == 4 {
            return None;
        }
        words.push(values::static_value(Some(child), source).ok()??);
    }
    Some(words)
}

fn relative_source(path: &str, source: &str) -> Option<String> {
    if source.starts_with(['/', '\\']) || source.contains(['$', ':', '\\']) {
        return None;
    }
    let mut parts = path
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    for component in source.split('/') {
        match component {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

fn push_guard(
    input: &FeatureFlagFileInput<'_>,
    guard: Node<'_>,
    read: &CodeFeatureFlagRecord,
    rows: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    check_fact_budget(rows.len())?;
    let mut row = record(
        input,
        &read.source_kind,
        &read.source_key,
        "guards_code",
        guard.start_byte(),
        guard.end_byte(),
    )?;
    row.metadata = read.metadata.clone();
    row.metadata.bindings.clear();
    row.metadata.declared_getter = None;
    row.metadata.read_usage_id = Some(read.usage_id.clone());
    rows.push(row);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_function_mutations_invalidate_prior_getter_bindings() {
        for mutation in [
            "source ./override.sh",
            "unset -f flag",
            "eval \"$OVERRIDE\"",
        ] {
            let source = format!(
                "flag() {{ printf '%s' \"${{FEATURE:-false}}\"; }}\n{mutation}\nvalue=$(flag)\nif [ \"$value\" = true ]; then echo ok; fi\n"
            );
            let rows =
                crate::code::feature_flags::syntax_test_support::extract("reader.sh", &source);
            assert!(
                rows.iter().any(|r| r.metadata.flow_incomplete.as_deref()
                    == Some("shell_function_binding_invalidated")),
                "{rows:?}"
            );
            assert!(!rows.iter().any(|r| r.edge_kind == "guards_code"
                && r.metadata.reference.as_deref() == Some("shell|reader.sh||flag")));
        }
    }
    #[test]
    fn shell_output_getters_connect_substitutions_and_local_conditions() {
        let rows = crate::code::feature_flags::syntax_test_support::extract(
            "settings.sh",
            "flag() { printf '%s' \"${FEATURE:-false}\"; }\nvalue=$(flag)\nif [ \"$value\" = true ]; then echo yes; fi\n",
        );
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FEATURE" && r.metadata.declared_getter.is_some()),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|r| r.edge_kind == "guards_code"
                && r.metadata.reference.as_deref() == Some("shell|settings.sh||flag")),
            "{rows:?}"
        );
    }
    #[test]
    fn shell_source_paths_do_not_escape_the_indexed_repository() {
        assert_eq!(relative_source("main.sh", "../outside.sh"), None);
        assert_eq!(
            relative_source("main.sh", "./settings.sh").as_deref(),
            Some("settings.sh")
        );
        assert_eq!(relative_source("main.sh", "$HOME/settings.sh"), None);
    }

    #[test]
    fn shell_sources_in_unexecuted_or_later_scopes_do_not_resolve_getters() {
        for source in [
            "load_settings() { source ./settings.sh; }\nvalue=$(flag)\n",
            "if [ -n \"$LOAD\" ]; then source ./settings.sh; fi\nvalue=$(flag)\n",
            "value=$(flag)\nsource ./settings.sh\n",
        ] {
            let rows =
                crate::code::feature_flags::syntax_test_support::extract("reader.sh", source);
            assert!(
                !rows.iter().any(|row| row
                    .metadata
                    .reference
                    .as_deref()
                    .is_some_and(|r| r.contains("settings.sh"))),
                "{rows:?}"
            );
        }
        let rows = crate::code::feature_flags::syntax_test_support::extract(
            "reader.sh",
            "source ./settings.sh\nvalue=$(flag)\n",
        );
        assert!(
            rows.iter()
                .any(|row| row.metadata.flow_incomplete.as_deref()
                    == Some("shell_source_working_directory_unknown"))
        );
    }
}
