use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareComponent, SoftwareDependencyUsage,
        SoftwareDependencyUsageInput, SoftwareGlobalRequest,
    },
    storage::StorageError,
};

const EXACT_MATCH_CONFIDENCE: u16 = 9500;
const NORMALIZED_MATCH_CONFIDENCE: u16 = 8500;
const HEURISTIC_MATCH_CONFIDENCE: u16 = 7000;

#[path = "dependency_usage_python.rs"]
mod python;

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    let had_usage_table = dependency_usage_table_exists(connection)?;
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS software_dependency_usages (
            usage_id TEXT PRIMARY KEY,
            component_id TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            package_name TEXT NOT NULL,
            language_id TEXT NOT NULL,
            module TEXT NOT NULL,
            target_hint TEXT,
            resolution_state TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_dependency_usages_scope
            ON software_dependency_usages(source_scope, language_id, ecosystem, package_name);
        ",
    )?;
    if !had_usage_table {
        mark_existing_projection_statuses_stale(connection)?;
    }

    Ok(())
}

pub(super) fn delete_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM software_dependency_usages WHERE source_scope = ?1",
        params![source_scope],
    )?;

    Ok(())
}

fn dependency_usage_table_exists(connection: &Connection) -> Result<bool, StorageError> {
    let exists = connection.query_row(
        "
        SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table'
              AND name = 'software_dependency_usages'
        )
        ",
        [],
        |row| row.get::<_, i64>(0),
    )?;

    Ok(exists != 0)
}

fn mark_existing_projection_statuses_stale(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE software_global_status
        SET stale = 1,
            last_error = COALESCE(
                last_error,
                'software dependency usage projection requires refresh'
            )
        ",
        [],
    )?;

    Ok(())
}

pub(super) fn derive_dependency_usages(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    components: &[SoftwareComponent],
) -> Result<Vec<SoftwareDependencyUsage>, StorageError> {
    let alias_keys = component_alias_keys(connection, source_scope)?;
    let index = DependencyMatchIndex::new(components, &alias_keys);
    if index.is_empty() {
        return Ok(Vec::new());
    }

    let imports = import_evidence(connection, source_scope)?;
    let python_local_modules = python::local_modules(connection, source_scope)?;
    let mut seen_usage_ids = BTreeSet::new();
    let mut usages = Vec::new();
    for import in imports {
        for candidate in import_match_candidates_with_python_locals(
            &import.language_id,
            &import.module,
            import.target_hint.as_deref(),
            &import.resolution_state,
            Some(&python_local_modules),
        ) {
            let matches = index.matching_components(
                &import.language_id,
                &candidate.value,
                &import.evidence_path,
            );
            for component_match in matches {
                let confidence = import
                    .confidence_basis_points
                    .min(candidate.confidence_basis_points)
                    .min(component_match.confidence_basis_points);
                let component = component_match.component;
                let usage = SoftwareDependencyUsage::new(SoftwareDependencyUsageInput {
                    component_id: component.component_id.clone(),
                    repository_id: import.repository_id.clone(),
                    source_scope: import.source_scope.clone(),
                    ecosystem: component.ecosystem.clone(),
                    package_name: component.name.clone(),
                    language_id: import.language_id.clone(),
                    module: import.module.clone(),
                    target_hint: import.target_hint.clone(),
                    resolution_state: import.resolution_state.clone(),
                    evidence_path: import.evidence_path.clone(),
                    evidence_line_range: import.evidence_line_range.clone(),
                    confidence_basis_points: confidence,
                    created_graph_version: graph_version,
                })
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
                if seen_usage_ids.insert(usage.usage_id.clone()) {
                    usages.push(usage);
                }
            }
        }
    }

    usages.sort_by(|left, right| {
        left.ecosystem
            .cmp(&right.ecosystem)
            .then_with(|| left.package_name.cmp(&right.package_name))
            .then_with(|| left.evidence_path.cmp(&right.evidence_path))
            .then_with(|| {
                left.evidence_line_range
                    .start
                    .cmp(&right.evidence_line_range.start)
            })
    });
    Ok(usages)
}

pub(super) fn insert_usage(
    connection: &Connection,
    usage: &SoftwareDependencyUsage,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_dependency_usages (
            usage_id, component_id, repository_id, source_scope, ecosystem, package_name,
            language_id, module, target_hint, resolution_state, evidence_path,
            evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
        params![
            usage.usage_id,
            usage.component_id,
            usage.repository_id,
            usage.source_scope,
            usage.ecosystem,
            usage.package_name,
            usage.language_id,
            usage.module,
            usage.target_hint,
            usage.resolution_state,
            usage.evidence_path,
            usage.evidence_line_range.start,
            usage.evidence_line_range.end,
            usage.confidence_basis_points,
            usage.created_graph_version.get(),
        ],
    )?;

    Ok(())
}

pub(super) fn usages_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareDependencyUsage>, StorageError> {
    let path_filter =
        super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter =
        super::language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let query = format!(
        "
        SELECT usage_id, component_id, repository_id, source_scope, ecosystem, package_name,
               language_id, module, target_hint, resolution_state, evidence_path,
               evidence_line_start, evidence_line_end, confidence_basis_points,
               created_graph_version
        FROM software_dependency_usages
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY ecosystem ASC, package_name ASC, evidence_path ASC, evidence_line_start ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), usage_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn import_evidence(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<ImportEvidence>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT imports.repository_id, imports.source_scope, files.language_id,
               imports.module, imports.target_hint, imports.resolution_state,
               imports.path, imports.line_start, imports.line_end,
               imports.confidence_basis_points
        FROM code_repository_imports imports
        JOIN code_repository_files files
          ON files.source_scope = imports.source_scope
         AND files.path = imports.path
        WHERE imports.source_scope = ?1
        ORDER BY files.language_id ASC, imports.module ASC, imports.path ASC, imports.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(ImportEvidence {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            language_id: row.get(2)?,
            module: row.get(3)?,
            target_hint: row.get(4)?,
            resolution_state: row.get(5)?,
            evidence_path: row.get(6)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(7)?,
                end: row.get(8)?,
            },
            confidence_basis_points: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareDependencyUsage> {
    Ok(SoftwareDependencyUsage {
        usage_id: row.get(0)?,
        component_id: row.get(1)?,
        repository_id: row.get(2)?,
        source_scope: row.get(3)?,
        ecosystem: row.get(4)?,
        package_name: row.get(5)?,
        language_id: row.get(6)?,
        module: row.get(7)?,
        target_hint: row.get(8)?,
        resolution_state: row.get(9)?,
        evidence_path: row.get(10)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(11)?,
            end: row.get(12)?,
        },
        confidence_basis_points: row.get(13)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(14)?),
    })
}

struct ImportEvidence {
    repository_id: String,
    source_scope: String,
    language_id: String,
    module: String,
    target_hint: Option<String>,
    resolution_state: String,
    evidence_path: String,
    evidence_line_range: RepositoryCodeRange,
    confidence_basis_points: u16,
}

struct DependencyMatchIndex<'a> {
    components: &'a [SoftwareComponent],
    by_language_key: BTreeMap<(String, String), Vec<(usize, u16)>>,
}

impl<'a> DependencyMatchIndex<'a> {
    fn new(components: &'a [SoftwareComponent], alias_keys: &ComponentAliasKeys) -> Self {
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

    fn is_empty(&self) -> bool {
        self.by_language_key.is_empty()
    }

    fn matching_components(
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
type ComponentAliasKeys = BTreeMap<ComponentEvidenceKey, Vec<MatchKey>>;

fn component_alias_keys(
    connection: &Connection,
    source_scope: &str,
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
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
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
    })?;
    let mut by_component = BTreeMap::new();
    for row in rows {
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

struct ComponentMatch<'a> {
    component: &'a SoftwareComponent,
    confidence_basis_points: u16,
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
struct MatchKey {
    value: String,
    confidence_basis_points: u16,
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
    )
}

fn import_match_candidates_with_python_locals(
    language_id: &str,
    module: &str,
    target_hint: Option<&str>,
    resolution_state: &str,
    python_local_modules: Option<&BTreeSet<String>>,
) -> Vec<MatchKey> {
    if import_uses_local_specifier(module, target_hint)
        || (resolution_state == "resolved" && language_id != "python")
    {
        return Vec::new();
    }

    let mut keys = Vec::new();
    if language_id == "python" {
        push_python_import_candidate_keys(
            &mut keys,
            module,
            resolution_state,
            python_local_modules,
        );
    } else {
        push_language_import_keys(&mut keys, language_id, module);
    }
    if language_id != "python"
        && matches!(resolution_state, "unresolved" | "external")
        && let Some(target_hint) = target_hint
    {
        push_language_import_keys(&mut keys, language_id, target_hint);
    }
    dedupe_keys_keep_highest_confidence(keys)
}

fn import_uses_local_specifier(module: &str, target_hint: Option<&str>) -> bool {
    let module = module.trim();
    module.starts_with(['.', '/'])
        || module
            .strip_prefix("from ")
            .is_some_and(|rest| rest.trim_start().starts_with(['.', '/']))
        || quoted_specs(module)
            .into_iter()
            .any(|spec| spec.trim().starts_with(['.', '/']))
        || target_hint.is_some_and(|hint| hint.trim().starts_with(['.', '/']))
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

fn push_language_import_keys(keys: &mut Vec<MatchKey>, language_id: &str, value: &str) {
    match language_id {
        "rust" => push_rust_import_keys(keys, value),
        "python" => push_python_import_keys(keys, value),
        "javascript" | "jsx" | "typescript" | "tsx" => push_package_import_keys(keys, value),
        "go" => push_go_import_keys(keys, value),
        "java" | "kotlin" | "scala" => push_jvm_import_keys(keys, value),
        "c" | "cpp" => push_native_import_keys(keys, value),
        _ => push_package_import_keys(keys, value),
    }
}

fn push_package_import_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let specs = quoted_specs(value);
    let specs = if specs.is_empty() {
        vec![value.trim()]
    } else {
        specs
    };
    for spec in specs {
        if let Some(root) = package_root(spec) {
            push_key(keys, &root, EXACT_MATCH_CONFIDENCE);
        }
    }
}

fn push_rust_import_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let mut value = value.trim().trim_end_matches(';').trim();
    value = value.strip_prefix("pub use ").unwrap_or(value);
    value = value.strip_prefix("use ").unwrap_or(value);
    value = value.strip_prefix("extern crate ").unwrap_or(value);
    let root = value
        .split([':', '{', ' ', ';'])
        .next()
        .unwrap_or_default()
        .trim();
    if !matches!(root, "" | "crate" | "self" | "super") {
        push_key(keys, root, EXACT_MATCH_CONFIDENCE);
    }
}

fn push_python_import_keys(keys: &mut Vec<MatchKey>, value: &str) {
    for module in python_import_modules(value) {
        push_python_module_key(keys, module);
    }
}

fn push_python_import_candidate_keys(
    keys: &mut Vec<MatchKey>,
    module: &str,
    resolution_state: &str,
    python_local_modules: Option<&BTreeSet<String>>,
) {
    let modules = python_import_modules(module);
    if resolution_state == "resolved" {
        let Some(python_local_modules) = python_local_modules else {
            return;
        };
        for module in modules {
            if python_module_is_local(python_local_modules, module) {
                continue;
            }
            push_python_module_key(keys, module);
        }
        return;
    }

    for module in modules {
        push_python_module_key(keys, module);
    }
}

fn python_import_modules(value: &str) -> Vec<&str> {
    let value = value.trim().trim_end_matches(';').trim();
    if let Some(rest) = value.strip_prefix("from ") {
        if let Some((module, _)) = rest.split_once(" import ") {
            return vec![module.trim()];
        }
        return Vec::new();
    }
    let rest = value.strip_prefix("import ").unwrap_or(value);
    rest.split(',')
        .map(|part| {
            part.trim()
                .split_once(" as ")
                .map_or(part.trim(), |(module, _)| module.trim())
        })
        .filter(|module| !module.is_empty())
        .collect()
}

fn python_module_is_local(local_modules: &BTreeSet<String>, module: &str) -> bool {
    local_modules.contains(&normalize_key(module))
}

fn push_python_module_key(keys: &mut Vec<MatchKey>, module: &str) {
    let Some(root) = python_module_root(module) else {
        return;
    };
    push_key(keys, root, EXACT_MATCH_CONFIDENCE);
    push_key(
        keys,
        &python_distribution_key(root),
        NORMALIZED_MATCH_CONFIDENCE,
    );
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

fn push_go_import_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let specs = quoted_specs(value);
    let specs = if specs.is_empty() {
        go_unquoted_import_specs(value)
    } else {
        specs
    };
    for spec in specs {
        push_go_package_keys(keys, spec);
    }
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

fn push_go_package_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let parts = value.split('/').collect::<Vec<_>>();
    let minimum_module_parts = if parts.first().is_some_and(|part| part.contains('.')) {
        2
    } else {
        3
    };
    if parts.len() < minimum_module_parts {
        push_key(keys, value, EXACT_MATCH_CONFIDENCE);
        return;
    }
    for end in (minimum_module_parts..=parts.len()).rev() {
        push_key(keys, &parts[..end].join("/"), EXACT_MATCH_CONFIDENCE);
    }
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

fn push_jvm_import_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let value = value
        .trim()
        .trim_end_matches(';')
        .trim_start_matches("import static ")
        .trim_start_matches("import ")
        .trim();
    push_key(keys, value, EXACT_MATCH_CONFIDENCE);
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() > 2 {
        for end in (2..parts.len()).rev() {
            push_key(keys, &parts[..end].join("."), NORMALIZED_MATCH_CONFIDENCE);
        }
    }
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

fn push_native_import_keys(keys: &mut Vec<MatchKey>, value: &str) {
    let mut specs = Vec::new();
    if let Some((_, rest)) = value.split_once('<')
        && let Some((header, _)) = rest.split_once('>')
    {
        specs.push(header);
    }
    if let Some((_, rest)) = value.split_once('"')
        && let Some((header, _)) = rest.split_once('"')
    {
        specs.push(header);
    }
    if specs.is_empty() {
        specs.push(value);
    }
    for spec in specs {
        if let Some(root) = spec.split('/').next().filter(|root| !root.is_empty()) {
            push_key(keys, root, NORMALIZED_MATCH_CONFIDENCE);
        }
        let stem = spec
            .rsplit('/')
            .next()
            .unwrap_or(spec)
            .trim_end_matches(".hpp")
            .trim_end_matches(".hxx")
            .trim_end_matches(".hh")
            .trim_end_matches(".h");
        push_key(keys, stem, HEURISTIC_MATCH_CONFIDENCE);
    }
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

fn normalize_key(value: &str) -> String {
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

fn quoted_specs(value: &str) -> Vec<&str> {
    let mut specs = Vec::new();
    let mut start = None::<usize>;
    let mut quote = '\0';
    for (index, character) in value.char_indices() {
        if start.is_none() && matches!(character, '"' | '\'' | '`') {
            start = Some(index + character.len_utf8());
            quote = character;
        } else if start.is_some() && character == quote {
            let spec_start = start.take().unwrap_or_default();
            specs.push(&value[spec_start..index]);
        }
    }

    specs
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
#[path = "dependency_usage_tests.rs"]
mod tests;
