use std::path::Path;

use crate::{
    code::SnapshotBuild,
    domain::{FrameworkEdgeKind, FrameworkKind, FrameworkNodeKind},
};

use super::{
    FrameworkFacts, FrameworkFileInput,
    scan::{framework_edge, framework_node, identifiers},
    template,
};

const MAX_SFC_REGIONS: usize = 16;

pub(super) fn extract(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    facts: &mut FrameworkFacts,
) -> Result<(), crate::code::CodeIndexError> {
    let component_name = vue_component_name(input.path);
    let component = framework_node(
        build,
        input,
        FrameworkKind::Vue,
        FrameworkNodeKind::Component,
        &component_name,
        None,
        0..input.content.len(),
    );
    let component_id = component.node_id.clone();
    facts.nodes.push(component);

    let root = input.syntax_root.ok_or_else(|| {
        crate::code::CodeIndexError::InvalidInput("Vue syntax tree is unavailable".to_owned())
    })?;
    let regions = sfc_regions(input.content, root);
    if regions.len() > MAX_SFC_REGIONS {
        return Err(crate::code::CodeIndexError::InvalidInput(format!(
            "Vue SFC region budget exceeded: {} > {MAX_SFC_REGIONS}",
            regions.len()
        )));
    }
    for region in regions {
        match region.name {
            "script" => extract_script_macros(
                build,
                input,
                region.content,
                region.content_start,
                &component_id,
                facts,
            ),
            "template" => {
                let node = framework_node(
                    build,
                    input,
                    FrameworkKind::Vue,
                    FrameworkNodeKind::Template,
                    "template",
                    None,
                    region.content_start..region.content_end,
                );
                let template_id = node.node_id.clone();
                facts.nodes.push(node);
                facts.edges.push(framework_edge(
                    build,
                    input,
                    FrameworkKind::Vue,
                    FrameworkEdgeKind::OwnsTemplate,
                    &component_id,
                    (Some(template_id.clone()), None),
                    region.content_start..region.content_end,
                ));
                template::extract_vue_template(
                    build,
                    input,
                    region.content,
                    region.content_start,
                    &template_id,
                    facts,
                );
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn script_mask(content: &str, root: tree_sitter::Node<'_>) -> Option<(String, bool)> {
    let regions = sfc_regions(content, root);
    let mut mask = content
        .bytes()
        .map(|byte| if byte == b'\n' { b'\n' } else { b' ' })
        .collect::<Vec<_>>();
    let mut typescript = false;
    let mut found = false;
    for region in regions.into_iter().filter(|region| region.name == "script") {
        found = true;
        typescript |= region.attributes.contains("lang=\"ts\"")
            || region.attributes.contains("lang='ts'")
            || region.attributes.contains("lang=\"tsx\"")
            || region.attributes.contains("lang='tsx'");
        mask[region.content_start..region.content_end].copy_from_slice(region.content.as_bytes());
    }
    found.then(|| {
        (
            String::from_utf8(mask).expect("Vue script mask remains valid UTF-8"),
            typescript,
        )
    })
}

fn extract_script_macros(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    script: &str,
    base_offset: usize,
    component_id: &str,
    facts: &mut FrameworkFacts,
) {
    for (macro_name, kind) in [
        ("defineProps", FrameworkNodeKind::Prop),
        ("defineEmits", FrameworkNodeKind::Emit),
        ("defineModel", FrameworkNodeKind::Model),
        ("defineSlots", FrameworkNodeKind::Slot),
    ] {
        for (macro_offset, _) in script.match_indices(macro_name) {
            let tail = script
                .get(macro_offset + macro_name.len()..)
                .unwrap_or_default();
            let declarations = macro_declarations(tail, kind);
            for (relative, name) in declarations {
                let absolute = base_offset + macro_offset + macro_name.len() + relative;
                let node = framework_node(
                    build,
                    input,
                    FrameworkKind::Vue,
                    kind,
                    &name,
                    None,
                    absolute..absolute + name.len(),
                );
                let node_id = node.node_id.clone();
                facts.nodes.push(node);
                facts.edges.push(framework_edge(
                    build,
                    input,
                    FrameworkKind::Vue,
                    FrameworkEdgeKind::Declares,
                    component_id,
                    (Some(node_id), None),
                    absolute..absolute + name.len(),
                ));
            }
        }
    }
}

fn macro_declarations(tail: &str, kind: FrameworkNodeKind) -> Vec<(usize, String)> {
    if kind == FrameworkNodeKind::Model {
        let name = quoted_argument(tail).unwrap_or("modelValue");
        return vec![(tail.find(name).unwrap_or_default(), name.to_owned())];
    }
    let bounded_end = tail
        .find(")")
        .or_else(|| tail.find("}>"))
        .unwrap_or(tail.len())
        .min(4_096);
    let bounded = tail.get(..bounded_end).unwrap_or_default();
    let mut names = Vec::new();
    for (offset, name) in identifiers(bounded) {
        if matches!(name, "string" | "number" | "boolean" | "true" | "false")
            || names.iter().any(|(_, existing)| existing == name)
        {
            continue;
        }
        names.push((offset, name.to_owned()));
    }
    for quote in ['\'', '"'] {
        for (offset, value) in quoted_values(bounded, quote) {
            if !names.iter().any(|(_, existing)| existing == value) {
                names.push((offset, value.to_owned()));
            }
        }
    }
    names
}

fn quoted_argument(tail: &str) -> Option<&str> {
    let open = tail.find('(')?;
    let rest = tail.get(open + 1..)?.trim_start();
    let quote = rest.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    rest.get(1..)?.split(quote).next()
}

fn quoted_values(content: &str, quote: char) -> impl Iterator<Item = (usize, &str)> {
    let mut cursor = 0usize;
    std::iter::from_fn(move || {
        let start = content.get(cursor..)?.find(quote)? + cursor;
        let end = content.get(start + 1..)?.find(quote)? + start + 1;
        cursor = end + quote.len_utf8();
        Some((start + 1, &content[start + 1..end]))
    })
}

struct SfcRegion<'a> {
    name: &'a str,
    attributes: &'a str,
    content: &'a str,
    content_start: usize,
    content_end: usize,
}

fn sfc_regions<'a>(content: &'a str, root: tree_sitter::Node<'_>) -> Vec<SfcRegion<'a>> {
    let mut regions = Vec::new();
    let mut cursor = root.walk();
    for element in root.named_children(&mut cursor) {
        let Some(open) = element
            .named_child(0)
            .filter(|node| node.kind() == "start_tag")
        else {
            continue;
        };
        let Some(tag) = open.named_child(0).filter(|node| node.kind() == "tag_name") else {
            continue;
        };
        let Some(attributes) = open
            .child(u32::try_from(open.child_count().saturating_sub(1)).unwrap_or(u32::MAX))
            .filter(|end| end.kind() == ">" && !end.is_missing())
            .and_then(|end| content.get(tag.end_byte()..end.start_byte()))
        else {
            continue;
        };
        let name = &content[tag.byte_range()];
        if !matches!(name, "script" | "template" | "style") {
            continue;
        }
        let mut children = element.walk();
        let Some(close) = element
            .named_children(&mut children)
            .find(|node| node.kind() == "end_tag")
        else {
            continue;
        };
        let content_start = open.end_byte();
        let content_end = close.start_byte();
        regions.push(SfcRegion {
            name,
            attributes,
            content: content.get(content_start..content_end).unwrap_or_default(),
            content_start,
            content_end,
        });
        if regions.len() > MAX_SFC_REGIONS {
            break;
        }
    }
    regions
}

fn vue_component_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("AnonymousComponent")
        .to_owned()
}
