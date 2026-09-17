//! Bounded explicit path evidence for detached type declarations.
use super::*;
use std::collections::BTreeSet;

pub(super) fn target_paths(
    root: Node<'_>,
    source: &str,
    path: &str,
    language: &str,
    hint: &str,
) -> Result<(Vec<String>, Option<String>), CodeIndexError> {
    let mut paths = BTreeSet::new();
    let mut imports = BTreeSet::new();
    let mut cursor = root.walk();
    for (index, node) in root.named_children(&mut cursor).enumerate() {
        if index >= MAX_NODES {
            return Err(incomplete("import node budget exceeded"));
        }
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default().trim();
        if language == "swift" && node.kind() == "import_declaration" {
            let words = raw.split_whitespace().take(4).collect::<Vec<_>>();
            if let ["import", "struct" | "class" | "enum" | "protocol", target] = words.as_slice()
                && target.rsplit('.').next() == Some(hint)
            {
                imports.insert((*target).to_owned());
            }
        }
        if language == "rust" && node.kind() == "use_declaration" {
            let Some(raw) = raw.trim_start_matches("pub ").strip_prefix("use ") else {
                continue;
            };
            let raw = raw.trim_end_matches(';').trim();
            let (target, alias) = raw.split_once(" as ").map_or(
                (raw, raw.rsplit("::").next().unwrap_or(raw)),
                |(target, alias)| (target.trim(), alias.trim()),
            );
            if alias != hint || target.contains(['{', '*']) {
                continue;
            }
            if rust_import_target(target) {
                imports.insert(target.replace("::", "."));
            }
        }

        if language == "cpp" && node.kind() == "preproc_include" {
            let Some(target) = node
                .child_by_field_name("path")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .and_then(|p| p.strip_prefix('"'))
                .and_then(|p| p.strip_suffix('"'))
            else {
                continue;
            };
            if let Some(target) = relative_path(path, target) {
                paths.insert(target);
            }
        }
        if paths.len() > 64 {
            return Err(incomplete("import target budget exceeded"));
        }
    }
    if language == "rust" && rust_import_target(&hint.replace('.', "::")) {
        imports.insert(hint.to_owned());
    }
    Ok((
        paths.into_iter().collect(),
        (imports.len() == 1)
            .then(|| imports.into_iter().next())
            .flatten(),
    ))
}

/// Explicit `mod` declarations establish file membership; file names alone do not.
pub(super) fn rust_module_declaration(
    node: Node<'_>,
    source: &str,
    path: &str,
) -> Option<CodeTypeOwner> {
    let name = syntax::name(node, source)?;
    let mut target_paths = Vec::new();
    if node.parent()?.kind() == "source_file" && node.child_by_field_name("body").is_none() {
        let dir = crate::domain::code_rust_modules::module_directory(path)?;
        target_paths = vec![format!("{dir}/{name}.rs"), format!("{dir}/{name}/mod.rs")];
        let mut previous = node.prev_named_sibling();
        for _ in 0..16 {
            let Some(attribute) = previous.filter(|n| n.kind() == "attribute_item") else {
                break;
            };
            let raw = attribute.utf8_text(source.as_bytes()).ok()?.trim();
            let raw = raw.strip_prefix("#[")?.strip_suffix(']')?.trim();
            if let Some(value) = raw
                .strip_prefix("path")
                .and_then(|s| s.trim().strip_prefix('='))
            {
                let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
                if value.len() > 1024 || value.contains(['"', '\\']) {
                    target_paths.clear();
                    break;
                }
                target_paths = relative_path(path, value).into_iter().collect();
            } else {
                // cfg/cfg_attr and procedural attributes may change module membership.
                target_paths.clear();
                break;
            }
            previous = attribute.prev_named_sibling();
        }
    }
    Some(CodeTypeOwner {
        static_dispatch: None,
        identity: format!("rust-module|{path}|{name}"),
        relation: "module_declaration".into(),
        target_hint: name,
        lookup_identity: None,
        basis: Some("rust_module".into()),
        resolution_state: Some(
            if target_paths.is_empty() {
                "unresolved"
            } else {
                "resolved"
            }
            .into(),
        ),
        target_paths,
        import_target: None,
        visibility: None,
    })
}

fn rust_import_target(target: &str) -> bool {
    let mut parts = target.split("::");
    matches!(parts.next(), Some("crate" | "self"))
        && parts.clone().count() >= 2
        && parts.clone().count() <= 17
        && parts
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

fn relative_path(path: &str, target: &str) -> Option<String> {
    if target.starts_with(['/', '\\']) || target.contains([':', '\\', '$']) {
        return None;
    }
    let mut parts = path
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}
