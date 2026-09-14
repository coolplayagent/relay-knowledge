use crate::{
    code::{CodeIndexError, SnapshotBuild},
    domain::{FrameworkEdgeKind, FrameworkKind, FrameworkNodeKind},
};

use super::{
    FrameworkFacts, FrameworkFileInput,
    scan::{expression_identifier, framework_edge, framework_node, identifiers},
};

pub(super) fn extract_angular_template(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    owner: Option<&str>,
    facts: &mut FrameworkFacts,
) -> Result<(), CodeIndexError> {
    let node = framework_node(
        build,
        input,
        FrameworkKind::Angular,
        FrameworkNodeKind::Template,
        input.path,
        None,
        0..input.content.len(),
    );
    let template_id = node.node_id.clone();
    facts.nodes.push(node);
    if let Some(owner) = owner {
        facts.edges.push(framework_edge(
            build,
            input,
            FrameworkKind::Angular,
            FrameworkEdgeKind::OwnsTemplate,
            owner,
            (Some(template_id.clone()), None),
            0..input.content.len(),
        ));
    }
    extract_angular_content(build, input, input.content, 0, &template_id, facts)
}

pub(super) fn extract_angular_content(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    content: &str,
    base_offset: usize,
    template_id: &str,
    facts: &mut FrameworkFacts,
) -> Result<(), CodeIndexError> {
    extract_tags(
        build,
        input,
        content,
        base_offset,
        FrameworkKind::Angular,
        template_id,
        facts,
    );
    extract_interpolations(
        build,
        input,
        content,
        base_offset,
        FrameworkKind::Angular,
        template_id,
        facts,
    );
    for keyword in ["@if", "@for", "@switch", "@defer", "@let"] {
        for (offset, _) in content.match_indices(keyword) {
            let node = framework_node(
                build,
                input,
                FrameworkKind::Angular,
                FrameworkNodeKind::ControlFlow,
                keyword.trim_start_matches('@'),
                None,
                base_offset + offset..base_offset + offset + keyword.len(),
            );
            let node_id = node.node_id.clone();
            facts.nodes.push(node);
            facts.edges.push(framework_edge(
                build,
                input,
                FrameworkKind::Angular,
                FrameworkEdgeKind::Declares,
                template_id,
                (Some(node_id), None),
                base_offset + offset..base_offset + offset + keyword.len(),
            ));
            if keyword == "@let" {
                let tail = content.get(offset + keyword.len()..).unwrap_or_default();
                if let Some((relative, name)) = identifiers(tail).next() {
                    push_template_variable(
                        build,
                        input,
                        FrameworkKind::Angular,
                        template_id,
                        TemplateVariable {
                            name,
                            detail: None,
                            absolute: base_offset + offset + keyword.len() + relative,
                        },
                        facts,
                    );
                }
            }
        }
    }
    Ok(())
}

pub(super) fn extract_vue_template(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    content: &str,
    base_offset: usize,
    template_id: &str,
    facts: &mut FrameworkFacts,
) {
    extract_tags(
        build,
        input,
        content,
        base_offset,
        FrameworkKind::Vue,
        template_id,
        facts,
    );
    extract_interpolations(
        build,
        input,
        content,
        base_offset,
        FrameworkKind::Vue,
        template_id,
        facts,
    );
}

fn extract_tags(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    content: &str,
    base_offset: usize,
    framework: FrameworkKind,
    template_id: &str,
    facts: &mut FrameworkFacts,
) {
    let mut cursor = 0usize;
    while let Some(relative_open) = content.get(cursor..).and_then(|tail| tail.find('<')) {
        let open = cursor + relative_open;
        let Some(close) = content
            .get(open..)
            .and_then(|tail| tail.find('>'))
            .map(|value| open + value)
        else {
            break;
        };
        let tag_source = content.get(open + 1..close).unwrap_or_default();
        let tag_source = tag_source.trim_start();
        if !tag_source.starts_with(['/', '!', '?']) {
            let tag = tag_source
                .split(|character: char| character.is_whitespace() || character == '/')
                .next()
                .unwrap_or_default();
            if component_tag(framework, tag) {
                facts.edges.push(framework_edge(
                    build,
                    input,
                    framework,
                    FrameworkEdgeKind::Renders,
                    template_id,
                    (None, Some(tag.to_owned())),
                    base_offset + open..base_offset + close + 1,
                ));
            }
            extract_attributes(
                build,
                input,
                tag_source,
                base_offset + open + 1,
                framework,
                template_id,
                facts,
            );
            if framework == FrameworkKind::Vue && tag == "slot" {
                let node = framework_node(
                    build,
                    input,
                    framework,
                    FrameworkNodeKind::Slot,
                    attribute_value(tag_source, "name").unwrap_or("default"),
                    None,
                    base_offset + open..base_offset + close + 1,
                );
                let node_id = node.node_id.clone();
                facts.nodes.push(node);
                facts.edges.push(framework_edge(
                    build,
                    input,
                    framework,
                    FrameworkEdgeKind::ProvidesSlot,
                    template_id,
                    (Some(node_id), None),
                    base_offset + open..base_offset + close + 1,
                ));
            }
        }
        cursor = close + 1;
    }
}

fn extract_attributes(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    tag: &str,
    base_offset: usize,
    framework: FrameworkKind,
    template_id: &str,
    facts: &mut FrameworkFacts,
) {
    for (offset, name, value) in quoted_attributes(tag) {
        let absolute = base_offset + offset;
        for (variable_offset, variable) in template_variables(framework, name, value) {
            push_template_variable(
                build,
                input,
                framework,
                template_id,
                TemplateVariable {
                    name: variable,
                    detail: Some(name.to_owned()),
                    absolute: absolute + variable_offset,
                },
                facts,
            );
        }
        let (edge_kind, control_flow) = match framework {
            FrameworkKind::Angular if name.starts_with("[(") => (FrameworkEdgeKind::Writes, false),
            FrameworkKind::Angular if name.starts_with('[') => {
                (FrameworkEdgeKind::BindsInput, false)
            }
            FrameworkKind::Angular if name.starts_with('(') => {
                (FrameworkEdgeKind::HandlesOutput, false)
            }
            FrameworkKind::Angular if name.starts_with('*') => {
                (FrameworkEdgeKind::UsesDirective, true)
            }
            FrameworkKind::Vue if name == "v-model" || name.starts_with("v-model:") => {
                (FrameworkEdgeKind::Writes, false)
            }
            FrameworkKind::Vue if name.starts_with(':') || name.starts_with("v-bind:") => {
                (FrameworkEdgeKind::BindsInput, false)
            }
            FrameworkKind::Vue if name.starts_with('@') || name.starts_with("v-on:") => {
                (FrameworkEdgeKind::HandlesOutput, false)
            }
            FrameworkKind::Vue if matches!(name, "v-if" | "v-for" | "v-show") => {
                (FrameworkEdgeKind::UsesDirective, true)
            }
            FrameworkKind::Vue if name.starts_with('#') || name.starts_with("v-slot") => {
                (FrameworkEdgeKind::ProvidesSlot, false)
            }
            _ => continue,
        };
        facts.edges.push(framework_edge(
            build,
            input,
            framework,
            edge_kind,
            template_id,
            (None, Some(name.to_owned())),
            absolute..absolute + name.len(),
        ));
        if control_flow {
            let node = framework_node(
                build,
                input,
                framework,
                FrameworkNodeKind::ControlFlow,
                name.trim_start_matches('*').trim_start_matches("v-"),
                Some(value.to_owned()),
                absolute..absolute + name.len(),
            );
            let node_id = node.node_id.clone();
            facts.nodes.push(node);
            facts.edges.push(framework_edge(
                build,
                input,
                framework,
                FrameworkEdgeKind::Declares,
                template_id,
                (Some(node_id), None),
                absolute..absolute + name.len(),
            ));
        }
        push_expression_reads(
            build,
            input,
            framework,
            template_id,
            value,
            absolute + name.len(),
            facts,
        );
    }
}

fn template_variables<'a>(
    framework: FrameworkKind,
    attribute: &'a str,
    value: &'a str,
) -> Vec<(usize, &'a str)> {
    match framework {
        FrameworkKind::Angular if attribute.starts_with('#') => {
            vec![(1, attribute.trim_start_matches('#'))]
        }
        FrameworkKind::Angular if attribute.starts_with("let-") => {
            vec![(4, attribute.trim_start_matches("let-"))]
        }
        FrameworkKind::Angular if attribute.starts_with('*') => identifiers(value)
            .filter(|(_, name)| *name != "let" && *name != "of" && expression_identifier(name))
            .take(1)
            .collect(),
        FrameworkKind::Vue if attribute == "v-for" => {
            let declarations = value
                .split_once(" in ")
                .or_else(|| value.split_once(" of "))
                .map_or(value, |(declarations, _)| declarations);
            identifiers(declarations)
                .filter(|(_, name)| expression_identifier(name))
                .collect()
        }
        FrameworkKind::Vue if attribute.starts_with('#') || attribute.starts_with("v-slot") => {
            identifiers(value)
                .filter(|(_, name)| expression_identifier(name))
                .collect()
        }
        _ => Vec::new(),
    }
}

struct TemplateVariable<'a> {
    name: &'a str,
    detail: Option<String>,
    absolute: usize,
}

fn push_template_variable(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    framework: FrameworkKind,
    template_id: &str,
    variable: TemplateVariable<'_>,
    facts: &mut FrameworkFacts,
) {
    if variable.name.is_empty()
        || facts.nodes.iter().any(|node| {
            node.kind == FrameworkNodeKind::TemplateVariable
                && node.name == variable.name
                && node.byte_range.start == u32::try_from(variable.absolute).unwrap_or(u32::MAX)
        })
    {
        return;
    }
    let node = framework_node(
        build,
        input,
        framework,
        FrameworkNodeKind::TemplateVariable,
        variable.name,
        variable.detail,
        variable.absolute..variable.absolute + variable.name.len(),
    );
    let node_id = node.node_id.clone();
    facts.nodes.push(node);
    facts.edges.push(framework_edge(
        build,
        input,
        framework,
        FrameworkEdgeKind::Declares,
        template_id,
        (Some(node_id), None),
        variable.absolute..variable.absolute + variable.name.len(),
    ));
}

fn extract_interpolations(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    content: &str,
    base_offset: usize,
    framework: FrameworkKind,
    template_id: &str,
    facts: &mut FrameworkFacts,
) {
    let mut cursor = 0usize;
    while let Some(start) = content
        .get(cursor..)
        .and_then(|tail| tail.find("{{"))
        .map(|value| cursor + value)
    {
        let Some(end) = content
            .get(start + 2..)
            .and_then(|tail| tail.find("}}"))
            .map(|value| start + 2 + value)
        else {
            break;
        };
        let expression = content.get(start + 2..end).unwrap_or_default();
        push_expression_reads(
            build,
            input,
            framework,
            template_id,
            expression,
            base_offset + start + 2,
            facts,
        );
        cursor = end + 2;
    }
}

fn push_expression_reads(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    framework: FrameworkKind,
    template_id: &str,
    expression: &str,
    base_offset: usize,
    facts: &mut FrameworkFacts,
) {
    for (relative, name) in identifiers(expression) {
        if !expression_identifier(name) {
            continue;
        }
        facts.edges.push(framework_edge(
            build,
            input,
            framework,
            FrameworkEdgeKind::Reads,
            template_id,
            (None, Some(name.to_owned())),
            base_offset + relative..base_offset + relative + name.len(),
        ));
    }
}

fn component_tag(framework: FrameworkKind, tag: &str) -> bool {
    match framework {
        FrameworkKind::Angular => {
            tag.contains('-') && !matches!(tag, "ng-container" | "ng-content" | "ng-template")
        }
        FrameworkKind::Vue => {
            tag.contains('-') || tag.chars().next().is_some_and(char::is_uppercase)
        }
    }
}

fn quoted_attributes(tag: &str) -> impl Iterator<Item = (usize, &str, &str)> {
    let mut cursor = tag.find(char::is_whitespace).unwrap_or(tag.len());
    std::iter::from_fn(move || {
        while tag
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if cursor >= tag.len() || tag.as_bytes().get(cursor) == Some(&b'/') {
            return None;
        }
        let name_start = cursor;
        while tag
            .as_bytes()
            .get(cursor)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'=' && *byte != b'/')
        {
            cursor += 1;
        }
        if name_start == cursor {
            return None;
        }
        let name = tag.get(name_start..cursor)?;
        while tag
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if tag.as_bytes().get(cursor) != Some(&b'=') {
            return Some((name_start, name, ""));
        }
        cursor += 1;
        while tag
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        let quote = *tag.as_bytes().get(cursor)?;
        if !matches!(quote, b'\'' | b'"') {
            while tag
                .as_bytes()
                .get(cursor)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                cursor += 1;
            }
            return Some((name_start, name, ""));
        }
        cursor += 1;
        let value_start = cursor;
        while tag
            .as_bytes()
            .get(cursor)
            .is_some_and(|byte| *byte != quote)
        {
            cursor += 1;
        }
        let value = tag.get(value_start..cursor).unwrap_or_default();
        cursor += usize::from(cursor < tag.len());
        Some((name_start, name, value))
    })
}

fn attribute_value<'a>(tag: &'a str, requested: &str) -> Option<&'a str> {
    quoted_attributes(tag).find_map(|(_, name, value)| (name == requested).then_some(value))
}
