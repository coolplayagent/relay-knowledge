use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use crate::{domain::SoftwareComponent, storage::StorageError};

const EXACT_MATCH_CONFIDENCE: u16 = 9500;
const NORMALIZED_MATCH_CONFIDENCE: u16 = 8500;
const HEURISTIC_MATCH_CONFIDENCE: u16 = 7000;
const MAX_IMPORT_MATCH_TEXT_BYTES: usize = 32 * 1_024;

pub(super) struct DependencyMatchIndex<'a> {
    components: &'a [SoftwareComponent],
    by_language_key: BTreeMap<(String, String), Vec<(usize, u16)>>,
}

impl<'a> DependencyMatchIndex<'a> {
    pub(super) fn new(
        components: &'a [SoftwareComponent],
        alias_keys: &ComponentAliasKeys,
    ) -> Self {
        let mut by_language_key = BTreeMap::<(String, String), Vec<(usize, u16)>>::new();
        let jvm_group_counts = jvm_declared_group_counts_by_owner(components);
        for (index, component) in components.iter().enumerate() {
            if component.relationship_state != "declared" || component.dependency_group == "bom" {
                continue;
            }
            let mut keys = component_match_keys(component);
            if let Some(component_alias_keys) = alias_keys.get(&component_evidence_key(component)) {
                keys.extend(component_alias_keys.iter().cloned());
            }
            push_unique_jvm_group_key(&mut keys, component, &jvm_group_counts);
            for key in dedupe_keys_keep_highest_confidence(keys) {
                by_language_key
                    .entry((component.language_id.clone(), key.value))
                    .or_default()
                    .push((index, key.confidence_basis_points));
            }
        }

        Self {
            components,
            by_language_key,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.by_language_key.is_empty()
    }

    pub(super) fn matching_components(
        &self,
        language_id: &str,
        key: &str,
        import_path: &str,
    ) -> Vec<ComponentMatch<'a>> {
        let matches = self
            .by_language_key
            .get(&(language_id.to_owned(), key.to_owned()))
            .into_iter()
            .flatten()
            .map(|(index, confidence)| ComponentMatch {
                component: &self.components[*index],
                confidence_basis_points: *confidence,
            })
            .collect::<Vec<_>>();
        matches_for_import_owner(import_path, matches)
    }
}

fn jvm_declared_group_counts_by_owner(
    components: &[SoftwareComponent],
) -> BTreeMap<(String, String), usize> {
    let mut artifacts_by_owner_group = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for component in components {
        if component.relationship_state != "declared"
            || !matches!(component.ecosystem.as_str(), "maven" | "gradle")
            || component.dependency_group == "bom"
        {
            continue;
        }
        if let Some((group, _)) = component.name.split_once(':') {
            artifacts_by_owner_group
                .entry((
                    manifest_owner_directory(component).to_owned(),
                    normalize_key(group),
                ))
                .or_default()
                .insert(normalize_key(&component.name));
        }
    }

    artifacts_by_owner_group
        .into_iter()
        .map(|(owner_group, artifacts)| (owner_group, artifacts.len()))
        .collect()
}

fn push_unique_jvm_group_key(
    keys: &mut Vec<MatchKey>,
    component: &SoftwareComponent,
    group_counts: &BTreeMap<(String, String), usize>,
) {
    if !matches!(component.ecosystem.as_str(), "maven" | "gradle") {
        return;
    }
    let Some((group, _)) = component.name.split_once(':') else {
        return;
    };
    let group_key = (
        manifest_owner_directory(component).to_owned(),
        normalize_key(group),
    );
    if group_counts.get(&group_key) == Some(&1) {
        push_key(keys, group, NORMALIZED_MATCH_CONFIDENCE);
    }
}

type ComponentEvidenceKey = (String, u32, String, String, String);
pub(super) type ComponentAliasKeys = BTreeMap<ComponentEvidenceKey, Vec<MatchKey>>;

pub(super) fn component_alias_keys(
    connection: &Connection,
    source_scope: &str,
    limit: usize,
) -> Result<ComponentAliasKeys, StorageError> {
    if !dependency_excerpt_column_exists(connection)? {
        return Ok(BTreeMap::new());
    }

    let mut statement = connection.prepare(
        "
        SELECT path, line_start, package_name, dependency_group, source_kind, excerpt
        FROM code_repository_dependencies
        WHERE source_scope = ?1
          AND ecosystem = 'cargo'
          AND is_lockfile = 0
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(
        params![source_scope, limit.saturating_add(1) as i64],
        |row| {
            Ok((
                (
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ),
                row.get::<_, String>(2)?,
                row.get::<_, String>(5)?,
            ))
        },
    )?;
    let mut by_component = BTreeMap::new();
    let mut evidence_count = 0_usize;
    for row in rows {
        evidence_count = evidence_count.saturating_add(1);
        if evidence_count > limit {
            return Err(StorageError::CapacityExceeded(format!(
                "software dependency component alias evidence exceeds the bounded limit {limit}"
            )));
        }
        let (key, package_name, excerpt) = row?;
        let keys = cargo_alias_match_keys(&package_name, &excerpt);
        if !keys.is_empty() {
            by_component.insert(key, keys);
        }
    }

    Ok(by_component)
}

fn dependency_excerpt_column_exists(connection: &Connection) -> Result<bool, StorageError> {
    let mut statement = connection.prepare("PRAGMA table_info(code_repository_dependencies)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == "excerpt" {
            return Ok(true);
        }
    }

    Ok(false)
}

fn component_evidence_key(component: &SoftwareComponent) -> ComponentEvidenceKey {
    (
        component.evidence_path.clone(),
        component.evidence_line_range.start,
        component.name.clone(),
        component.dependency_group.clone(),
        component.source_kind.clone(),
    )
}

fn cargo_alias_match_keys(package_name: &str, excerpt: &str) -> Vec<MatchKey> {
    let Some((alias, _)) = excerpt.split_once('=') else {
        return Vec::new();
    };
    let alias = alias.trim().trim_matches('"').trim_matches('\'');
    if alias.is_empty() || normalize_key(alias) == normalize_key(package_name) {
        return Vec::new();
    }

    let mut keys = Vec::new();
    push_key(&mut keys, alias, EXACT_MATCH_CONFIDENCE);
    push_key(
        &mut keys,
        &alias.replace('-', "_"),
        NORMALIZED_MATCH_CONFIDENCE,
    );
    dedupe_keys_keep_highest_confidence(keys)
}

pub(super) struct ComponentMatch<'a> {
    pub(super) component: &'a SoftwareComponent,
    pub(super) confidence_basis_points: u16,
}

fn matches_for_import_owner<'a>(
    import_path: &str,
    matches: Vec<ComponentMatch<'a>>,
) -> Vec<ComponentMatch<'a>> {
    let owned = matches
        .into_iter()
        .filter(|candidate| manifest_owns_import(candidate.component, import_path))
        .collect::<Vec<_>>();
    let Some(max_depth) = owned
        .iter()
        .map(|candidate| manifest_owner_directory(candidate.component).len())
        .max()
    else {
        return Vec::new();
    };

    owned
        .into_iter()
        .filter(|candidate| manifest_owner_directory(candidate.component).len() == max_depth)
        .collect()
}

fn manifest_owns_import(component: &SoftwareComponent, import_path: &str) -> bool {
    let directory = manifest_owner_directory(component);
    directory.is_empty()
        || import_path == directory
        || import_path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn manifest_owner_directory(component: &SoftwareComponent) -> &str {
    if component.ecosystem == "python"
        && component.source_kind == "requirements.txt"
        && let Some(owner) = python_requirements_owner_directory(&component.evidence_path)
    {
        return owner;
    }

    manifest_directory(component)
}

fn manifest_directory(component: &SoftwareComponent) -> &str {
    component
        .evidence_path
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory)
}

fn python_requirements_owner_directory(path: &str) -> Option<&str> {
    let (directory, file_name) = path.rsplit_once('/').map_or(("", path), |parts| parts);
    if file_name.starts_with("requirements") {
        return Some(directory);
    }
    if path.strip_prefix("requirements/").is_some() {
        return Some("");
    }
    path.find("/requirements/").map(|index| &path[..index])
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct MatchKey {
    pub(super) value: String,
    pub(super) confidence_basis_points: u16,
}

fn component_match_keys(component: &SoftwareComponent) -> Vec<MatchKey> {
    let name = component.name.trim();
    let mut keys = Vec::new();
    push_key(&mut keys, name, EXACT_MATCH_CONFIDENCE);
    match component.ecosystem.as_str() {
        "cargo" => push_key(
            &mut keys,
            &name.replace('-', "_"),
            NORMALIZED_MATCH_CONFIDENCE,
        ),
        "python" => push_key(
            &mut keys,
            &python_distribution_key(name),
            NORMALIZED_MATCH_CONFIDENCE,
        ),
        "go" => push_key(&mut keys, name, EXACT_MATCH_CONFIDENCE),
        "maven" | "gradle" => push_jvm_component_keys(&mut keys, name),
        "conan" | "cmake" => push_native_component_keys(&mut keys, name),
        _ => {}
    }

    dedupe_keys_keep_highest_confidence(keys)
}

#[cfg(test)]
fn import_match_candidates(
    language_id: &str,
    module: &str,
    target_hint: Option<&str>,
    resolution_state: &str,
) -> Vec<MatchKey> {
    import_match_candidates_with_python_locals(
        language_id,
        module,
        target_hint,
        resolution_state,
        None,
        128,
    )
    .expect("unit-test import candidate text should fit bounded inputs")
}

pub(super) fn import_match_candidates_with_python_locals(
    language_id: &str,
    module: &str,
    target_hint: Option<&str>,
    resolution_state: &str,
    python_local_modules: Option<&BTreeSet<String>>,
    limit: usize,
) -> Result<Vec<MatchKey>, StorageError> {
    let distinct_target_hint = target_hint.filter(|target_hint| *target_hint != module);
    let input_bytes = module
        .len()
        .saturating_add(distinct_target_hint.map_or(0, str::len));
    if input_bytes > MAX_IMPORT_MATCH_TEXT_BYTES {
        return Err(StorageError::CapacityExceeded(format!(
            "software dependency import match text bytes {input_bytes} exceed the bounded limit {MAX_IMPORT_MATCH_TEXT_BYTES}"
        )));
    }
    if import_uses_local_specifier(module, distinct_target_hint)
        || (resolution_state == "resolved" && language_id != "python")
    {
        return Ok(Vec::new());
    }

    let mut keys = BoundedMatchKeys::new(limit);
    if language_id == "python" {
        for python_module in python_import_modules_iter(module) {
            if resolution_state == "resolved"
                && (python_local_modules.is_none()
                    || python_local_modules
                        .is_some_and(|modules| python_module_is_local(modules, python_module)))
            {
                continue;
            }
            if let Some(root) = python_module_root(python_module) {
                keys.insert(root, EXACT_MATCH_CONFIDENCE)?;
                keys.insert(&python_distribution_key(root), NORMALIZED_MATCH_CONFIDENCE)?;
            }
        }
    } else {
        visit_language_import_keys(language_id, module, |value, confidence| {
            keys.insert(value, confidence)
        })?;
    }
    if language_id != "python"
        && matches!(resolution_state, "unresolved" | "external")
        && let Some(target_hint) = distinct_target_hint
    {
        visit_language_import_keys(language_id, target_hint, |value, confidence| {
            keys.insert(value, confidence)
        })?;
    }
    Ok(keys.into_vec())
}

struct BoundedMatchKeys {
    by_value: BTreeMap<String, u16>,
    limit: usize,
}

impl BoundedMatchKeys {
    fn new(limit: usize) -> Self {
        Self {
            by_value: BTreeMap::new(),
            limit,
        }
    }

    fn insert(&mut self, value: &str, confidence: u16) -> Result<(), StorageError> {
        let value = normalize_key(value);
        if value.is_empty() || value.starts_with('.') || value.starts_with('/') {
            return Ok(());
        }
        if let Some(current) = self.by_value.get_mut(&value) {
            *current = (*current).max(confidence);
            return Ok(());
        }
        if self.by_value.len() >= self.limit {
            return Err(StorageError::CapacityExceeded(format!(
                "software dependency match candidates exceed the per-import bounded limit {}",
                self.limit
            )));
        }
        self.by_value.insert(value, confidence);
        Ok(())
    }

    fn into_vec(self) -> Vec<MatchKey> {
        self.by_value
            .into_iter()
            .map(|(value, confidence_basis_points)| MatchKey {
                value,
                confidence_basis_points,
            })
            .collect()
    }
}

fn visit_language_import_keys(
    language_id: &str,
    value: &str,
    mut visit: impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    match language_id {
        "python" => visit_python_import_keys(value, &mut visit),
        "java" | "kotlin" | "scala" => visit_jvm_import_keys(value, &mut visit),
        "go" => visit_go_import_keys(value, &mut visit),
        "rust" => visit_rust_import_keys(value, &mut visit),
        "javascript" | "jsx" | "typescript" | "tsx" => visit_package_import_keys(value, &mut visit),
        "c" | "cpp" => visit_native_import_keys(value, &mut visit),
        _ => visit_package_import_keys(value, &mut visit),
    }
}

fn visit_rust_import_keys(
    value: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut value = value.trim().trim_end_matches(';').trim();
    value = value.strip_prefix("pub use ").unwrap_or(value);
    value = value.strip_prefix("use ").unwrap_or(value);
    value = value.strip_prefix("extern crate ").unwrap_or(value);
    let root = value
        .split([':', '{', ' ', ';'])
        .next()
        .unwrap_or_default()
        .trim();
    if matches!(root, "" | "crate" | "self" | "super") {
        return Ok(());
    }
    visit(root, EXACT_MATCH_CONFIDENCE)
}

fn visit_package_import_keys(
    value: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut found = false;
    visit_quoted_specs(value, |spec| {
        found = true;
        if let Some(root) = package_root(spec) {
            visit(&root, EXACT_MATCH_CONFIDENCE)?;
        }
        Ok(())
    })?;
    if !found && let Some(root) = package_root(value.trim()) {
        visit(&root, EXACT_MATCH_CONFIDENCE)?;
    }
    Ok(())
}

fn visit_python_import_keys(
    value: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    for module in python_import_modules_iter(value) {
        if let Some(root) = python_module_root(module) {
            visit(root, EXACT_MATCH_CONFIDENCE)?;
            let distribution = python_distribution_key(root);
            visit(&distribution, NORMALIZED_MATCH_CONFIDENCE)?;
        }
    }
    Ok(())
}

fn visit_jvm_import_keys(
    value: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let value = value
        .trim()
        .trim_end_matches(';')
        .trim_start_matches("import static ")
        .trim_start_matches("import ")
        .trim();
    visit(value, EXACT_MATCH_CONFIDENCE)?;
    let separator_count = value.bytes().filter(|byte| *byte == b'.').count();
    if separator_count > 1 {
        let mut end = value.len();
        while let Some(dot) = value[..end].rfind('.') {
            end = dot;
            if value[..end].contains('.') {
                visit(&value[..end], NORMALIZED_MATCH_CONFIDENCE)?;
            }
        }
    }
    Ok(())
}

fn visit_go_import_keys(
    value: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut found = false;
    visit_quoted_specs(value, |spec| {
        found = true;
        visit_go_package_keys(spec, visit)
    })?;
    if !found {
        for spec in go_unquoted_import_specs(value) {
            visit_go_package_keys(spec, visit)?;
        }
    }
    Ok(())
}

fn visit_native_import_keys(
    value: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let angle = value
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>').map(|(header, _)| header));
    let quoted = value
        .split_once('"')
        .and_then(|(_, rest)| rest.split_once('"').map(|(header, _)| header));
    let mut found = false;
    for spec in angle.into_iter().chain(quoted) {
        found = true;
        visit_native_spec(spec, visit)?;
    }
    if !found {
        visit_native_spec(value, visit)?;
    }
    Ok(())
}

fn visit_native_spec(
    spec: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    if let Some(root) = spec.split('/').next().filter(|root| !root.is_empty()) {
        visit(root, NORMALIZED_MATCH_CONFIDENCE)?;
    }
    let stem = spec
        .rsplit('/')
        .next()
        .unwrap_or(spec)
        .trim_end_matches(".hpp")
        .trim_end_matches(".hxx")
        .trim_end_matches(".hh")
        .trim_end_matches(".h");
    visit(stem, HEURISTIC_MATCH_CONFIDENCE)
}

fn visit_quoted_specs(
    value: &str,
    mut visit: impl FnMut(&str) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut start = None::<usize>;
    let mut quote = '\0';
    for (index, character) in value.char_indices() {
        if start.is_none() && matches!(character, '"' | '\'' | '`') {
            start = Some(index + character.len_utf8());
            quote = character;
        } else if start.is_some() && character == quote {
            let spec_start = start.take().unwrap_or_default();
            visit(&value[spec_start..index])?;
        }
    }
    Ok(())
}

fn visit_go_package_keys(
    value: &str,
    visit: &mut impl FnMut(&str, u16) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let part_count = value.split('/').count();
    let minimum = if value
        .split('/')
        .next()
        .is_some_and(|part| part.contains('.'))
    {
        2
    } else {
        3
    };
    if part_count < minimum {
        return visit(value, EXACT_MATCH_CONFIDENCE);
    }
    let mut end = value.len();
    let mut current_parts = part_count;
    while current_parts >= minimum {
        visit(&value[..end], EXACT_MATCH_CONFIDENCE)?;
        if current_parts == minimum {
            break;
        }
        end = value[..end].rfind('/').unwrap_or(0);
        current_parts -= 1;
    }
    Ok(())
}

fn python_import_modules_iter(value: &str) -> Box<dyn Iterator<Item = &str> + '_> {
    let value = value.trim().trim_end_matches(';').trim();
    if let Some(rest) = value.strip_prefix("from ") {
        return Box::new(
            rest.split_once(" import ")
                .map(|(module, _)| module.trim())
                .into_iter(),
        );
    }
    let rest = value.strip_prefix("import ").unwrap_or(value);
    Box::new(rest.split(',').filter_map(|part| {
        let module = part
            .trim()
            .split_once(" as ")
            .map_or(part.trim(), |(module, _)| module.trim());
        (!module.is_empty()).then_some(module)
    }))
}

fn import_uses_local_specifier(module: &str, target_hint: Option<&str>) -> bool {
    let module = module.trim();
    module.starts_with(['.', '/'])
        || module
            .strip_prefix("from ")
            .is_some_and(|rest| rest.trim_start().starts_with(['.', '/']))
        || quoted_spec_is_local(module)
        || target_hint.is_some_and(|hint| hint.trim().starts_with(['.', '/']))
}

fn quoted_spec_is_local(value: &str) -> bool {
    let mut local = false;
    let _ = visit_quoted_specs(value, |spec| {
        local |= spec.trim().starts_with(['.', '/']);
        Ok(())
    });
    local
}

fn dedupe_keys_keep_highest_confidence(keys: Vec<MatchKey>) -> Vec<MatchKey> {
    let mut by_value = BTreeMap::<String, u16>::new();
    for key in keys {
        by_value
            .entry(key.value)
            .and_modify(|confidence| *confidence = (*confidence).max(key.confidence_basis_points))
            .or_insert(key.confidence_basis_points);
    }

    by_value
        .into_iter()
        .map(|(value, confidence_basis_points)| MatchKey {
            value,
            confidence_basis_points,
        })
        .collect()
}

fn python_module_is_local(local_modules: &BTreeSet<String>, module: &str) -> bool {
    local_modules.contains(&normalize_key(module))
}

fn python_module_root(module: &str) -> Option<&str> {
    let module = module.trim();
    if module.starts_with('.') {
        return None;
    }
    module
        .split('.')
        .next()
        .map(str::trim)
        .filter(|root| !root.is_empty())
}

fn go_unquoted_import_specs(value: &str) -> Vec<&str> {
    let value = value
        .trim()
        .trim_end_matches(';')
        .strip_prefix("import ")
        .unwrap_or(value.trim())
        .trim();
    value
        .split_whitespace()
        .last()
        .map(|spec| vec![spec.trim_matches(['"', '\'', '`'])])
        .unwrap_or_default()
}

fn push_jvm_component_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let (group, artifact) = value.split_once(':').unwrap_or(("", value));
    push_key(keys, artifact, NORMALIZED_MATCH_CONFIDENCE);
    push_key(
        keys,
        &artifact.replace('-', "."),
        NORMALIZED_MATCH_CONFIDENCE,
    );
    if !group.is_empty()
        && let Some(package_key) = jvm_artifact_package_key(group, artifact)
    {
        push_key(keys, &package_key, NORMALIZED_MATCH_CONFIDENCE);
    }
}

fn jvm_artifact_package_key(group: &str, artifact: &str) -> Option<String> {
    let artifact_key = artifact.replace(['-', '_'], ".");
    let tokens = artifact_key
        .split('.')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let group_tail = group.rsplit('.').next().unwrap_or(group);
    let suffix = if tokens
        .first()
        .is_some_and(|token| group_tail == *token || group_tail.starts_with(*token))
    {
        &tokens[1..]
    } else {
        tokens.as_slice()
    };
    if suffix.is_empty() {
        return None;
    }

    Some(format!("{group}.{}", suffix.join(".")))
}

fn push_native_component_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let package = value.split('/').next().unwrap_or(value);
    push_key(keys, package, NORMALIZED_MATCH_CONFIDENCE);
    push_key(
        keys,
        &package.replace(['-', '_'], ""),
        HEURISTIC_MATCH_CONFIDENCE,
    );
}

fn push_key(keys: &mut Vec<MatchKey>, value: &str, confidence_basis_points: u16) {
    let value = normalize_key(value);
    if !value.is_empty() && !value.starts_with('.') && !value.starts_with('/') {
        keys.push(MatchKey {
            value,
            confidence_basis_points,
        });
    }
}

pub(super) fn normalize_key(value: &str) -> String {
    value
        .trim()
        .trim_matches(['"', '\'', '`', '<', '>', ';'])
        .to_ascii_lowercase()
}

fn python_distribution_key(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '_' | '.' => '-',
            other => other,
        })
        .collect::<String>()
}

fn package_root(spec: &str) -> Option<String> {
    let spec = spec.trim();
    if spec.is_empty() || spec.starts_with(['.', '/']) {
        return None;
    }
    if spec.starts_with('@') {
        let mut parts = spec.split('/');
        let scope = parts.next()?;
        let package = parts.next()?;
        return Some(format!("{scope}/{package}"));
    }

    spec.split('/').next().map(str::to_owned)
}

#[cfg(test)]
#[path = "matching_tests.rs"]
mod tests;
