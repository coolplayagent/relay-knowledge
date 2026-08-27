use crate::{
    code::SnapshotBuild,
    domain::{FrameworkEdgeKind, FrameworkKind, FrameworkNodeKind},
};

use super::{
    FrameworkFacts, FrameworkFileInput,
    scan::{
        balanced_region, framework_edge, framework_node, quoted_property, relative_module_path,
    },
    template,
};

const ANGULAR_DECORATORS: [(&str, FrameworkNodeKind); 3] = [
    ("Component", FrameworkNodeKind::Component),
    ("Directive", FrameworkNodeKind::Directive),
    ("Pipe", FrameworkNodeKind::Pipe),
];

pub(super) fn looks_like_template(path: &str, content: &str) -> bool {
    path.ends_with(".component.html")
        || content.contains("[(")
        || content.contains("@if")
        || content.contains("@for")
        || content.contains("*ng")
}

pub(super) fn extract(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    facts: &mut FrameworkFacts,
) -> Result<(), crate::code::CodeIndexError> {
    for (decorator, kind) in ANGULAR_DECORATORS {
        let mut search_start = 0usize;
        let marker = format!("@{decorator}");
        while let Some(relative_start) = input
            .content
            .get(search_start..)
            .and_then(|text| text.find(&marker))
        {
            let decorator_start = search_start + relative_start;
            let Some(open) = input
                .content
                .get(decorator_start + marker.len()..)
                .and_then(|text| text.find('('))
                .map(|offset| decorator_start + marker.len() + offset)
            else {
                break;
            };
            let Some(call_end) = balanced_region(input.content, open, b'(', b')') else {
                break;
            };
            let metadata = input
                .content
                .get(open + 1..call_end.saturating_sub(1))
                .unwrap_or_default();
            let class_name = class_name_after(input.content, call_end).unwrap_or(decorator);
            let component_end = class_end_after(input.content, call_end).unwrap_or(call_end);
            let node = framework_node(
                build,
                input,
                FrameworkKind::Angular,
                kind,
                class_name,
                quoted_property(metadata, "selector").map(|(selector, _, _)| selector),
                decorator_start..component_end,
            );
            let component_id = node.node_id.clone();
            facts.nodes.push(node);

            if kind == FrameworkNodeKind::Component {
                extract_component_metadata(build, input, metadata, open + 1, &component_id, facts)?;
                extract_class_bindings(build, input, call_end, component_end, &component_id, facts);
            }
            search_start = call_end;
        }
    }
    Ok(())
}

fn extract_component_metadata(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    metadata: &str,
    metadata_offset: usize,
    component_id: &str,
    facts: &mut FrameworkFacts,
) -> Result<(), crate::code::CodeIndexError> {
    if let Some((template_path, start, end)) = quoted_property(metadata, "templateUrl") {
        let target_path = relative_module_path(input.path, &template_path);
        facts.edges.push(framework_edge(
            build,
            input,
            FrameworkKind::Angular,
            FrameworkEdgeKind::OwnsTemplate,
            component_id,
            (None, Some(target_path)),
            metadata_offset + start..metadata_offset + end,
        ));
    }
    if let Some((inline_template, start, end)) = quoted_property(metadata, "template") {
        let absolute_start = metadata_offset + start + 1;
        let template_node = framework_node(
            build,
            input,
            FrameworkKind::Angular,
            FrameworkNodeKind::Template,
            "inline_template",
            None,
            absolute_start..metadata_offset + end.saturating_sub(1),
        );
        let template_id = template_node.node_id.clone();
        facts.nodes.push(template_node);
        facts.edges.push(framework_edge(
            build,
            input,
            FrameworkKind::Angular,
            FrameworkEdgeKind::OwnsTemplate,
            component_id,
            (Some(template_id.clone()), None),
            metadata_offset + start..metadata_offset + end,
        ));
        template::extract_angular_content(
            build,
            input,
            &inline_template,
            absolute_start,
            &template_id,
            facts,
        )?;
    }
    if let Some(imports) = array_property(metadata, "imports") {
        for (relative, name) in super::scan::identifiers(imports) {
            if !super::scan::expression_identifier(name) {
                continue;
            }
            facts.edges.push(framework_edge(
                build,
                input,
                FrameworkKind::Angular,
                FrameworkEdgeKind::Imports,
                component_id,
                (None, Some(name.to_owned())),
                metadata_offset + relative..metadata_offset + relative + name.len(),
            ));
        }
    }
    Ok(())
}

fn extract_class_bindings(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    class_start: usize,
    class_end: usize,
    component_id: &str,
    facts: &mut FrameworkFacts,
) {
    let class = input
        .content
        .get(class_start..class_end)
        .unwrap_or_default();
    for (marker, kind) in [
        ("@Input", FrameworkNodeKind::Input),
        ("@Output", FrameworkNodeKind::Output),
        ("input(", FrameworkNodeKind::Input),
        ("input.required", FrameworkNodeKind::Input),
        ("output(", FrameworkNodeKind::Output),
    ] {
        for (line_offset, line) in lines_with_offsets(class) {
            if !line.contains(marker) {
                continue;
            }
            let Some(name) = binding_member_name(line, marker) else {
                continue;
            };
            let absolute = class_start + line_offset + line.find(name).unwrap_or_default();
            let node = framework_node(
                build,
                input,
                FrameworkKind::Angular,
                kind,
                name,
                None,
                absolute..absolute + name.len(),
            );
            let node_id = node.node_id.clone();
            facts.nodes.push(node);
            facts.edges.push(framework_edge(
                build,
                input,
                FrameworkKind::Angular,
                FrameworkEdgeKind::Declares,
                component_id,
                (Some(node_id), None),
                absolute..absolute + name.len(),
            ));
        }
    }
}

fn class_name_after(content: &str, offset: usize) -> Option<&str> {
    let tail = content.get(offset..)?;
    let class = tail.find("class ")? + "class ".len();
    tail.get(class..)?
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .next()
        .filter(|name| !name.is_empty())
}

fn class_end_after(content: &str, offset: usize) -> Option<usize> {
    let open = content.get(offset..)?.find('{')? + offset;
    balanced_region(content, open, b'{', b'}')
}

fn array_property<'a>(content: &'a str, property: &str) -> Option<&'a str> {
    let start = content.find(property)? + property.len();
    let open = content.get(start..)?.find('[')? + start;
    let end = balanced_region(content, open, b'[', b']')?;
    content.get(open + 1..end.saturating_sub(1))
}

fn lines_with_offsets(content: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0usize;
    content.split_inclusive('\n').map(move |line| {
        let current = offset;
        offset += line.len();
        (current, line)
    })
}

fn binding_member_name<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let equals = line.find('=')?;
    if marker.starts_with('@') {
        let after = line.find(')')? + 1;
        return line.get(after..equals)?.split_whitespace().last();
    }
    line.get(..equals)?.split_whitespace().last()
}
