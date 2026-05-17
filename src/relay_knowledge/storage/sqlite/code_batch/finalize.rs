use std::collections::BTreeMap;

use rusqlite::{Transaction, params};

use crate::{domain::RepositoryCodeRange, storage::StorageError};

#[path = "finalize_go_imports.rs"]
mod go_imports;
#[path = "finalize_search.rs"]
mod search_documents;

pub(super) fn resolve_scope(
    transaction: &Transaction<'_>,
    source_scope: &str,
    repository_id: &str,
) -> Result<(), StorageError> {
    resolve_references(transaction, source_scope)?;
    search_documents::rebuild_reference_search_documents(transaction, source_scope)?;
    resolve_imports(transaction, source_scope)?;
    rebuild_calls(transaction, source_scope, repository_id)
}

fn resolve_references(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        UPDATE code_repository_references
        SET target_symbol_snapshot_id = NULL,
            target_hint = name,
            resolution_state = 'unresolved',
            confidence_basis_points = 2500,
            confidence_tier = 'ambiguous'
        WHERE source_scope = ?1
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        UPDATE code_repository_references AS reference
        SET target_symbol_snapshot_id = (
                SELECT symbol.symbol_snapshot_id
                FROM code_repository_symbols AS symbol
                WHERE symbol.source_scope = reference.source_scope
                  AND symbol.name = reference.name
                LIMIT 1
            ),
            resolution_state = 'resolved',
            confidence_basis_points = 8000,
            confidence_tier = 'inferred'
        WHERE reference.source_scope = ?1
          AND (
                SELECT COUNT(*)
                FROM code_repository_symbols AS symbol
                WHERE symbol.source_scope = reference.source_scope
                  AND symbol.name = reference.name
            ) = 1
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        UPDATE code_repository_references AS reference
        SET target_symbol_snapshot_id = (
                SELECT symbol.symbol_snapshot_id
                FROM code_repository_symbols AS symbol
                WHERE symbol.source_scope = reference.source_scope
                  AND symbol.name = reference.name
                  AND symbol.path = reference.path
                LIMIT 1
            ),
            resolution_state = 'resolved',
            confidence_basis_points = 8000,
            confidence_tier = 'inferred'
        WHERE reference.source_scope = ?1
          AND reference.resolution_state != 'resolved'
          AND (
                SELECT COUNT(*)
                FROM code_repository_symbols AS symbol
                WHERE symbol.source_scope = reference.source_scope
                  AND symbol.name = reference.name
                  AND symbol.path = reference.path
            ) = 1
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        UPDATE code_repository_references AS reference
        SET resolution_state = 'ambiguous',
            confidence_basis_points = 5000,
            confidence_tier = 'ambiguous'
        WHERE reference.source_scope = ?1
          AND reference.resolution_state = 'unresolved'
          AND EXISTS (
                SELECT 1
                FROM code_repository_symbols AS symbol
                WHERE symbol.source_scope = reference.source_scope
                  AND symbol.name = reference.name
            )
        ",
        params![source_scope],
    )?;

    Ok(())
}

fn resolve_imports(transaction: &Transaction<'_>, source_scope: &str) -> Result<(), StorageError> {
    let files = load_file_languages(transaction, source_scope)?;
    let module_paths = module_path_index(files.keys());
    let imports = load_import_keys(transaction, source_scope)?;
    let symbols_by_name = if imports.iter().any(|import| {
        let statement = import.module.trim();
        match files.get(&import.path).map(String::as_str) {
            Some("python") => statement.starts_with("from "),
            Some("java") => statement
                .trim_end_matches(';')
                .strip_prefix("import ")
                .is_some_and(|body| body.trim_start().starts_with("static ")),
            _ => false,
        }
    }) {
        let mut symbols_by_name = BTreeMap::<String, Vec<SymbolKey>>::new();
        for symbol in load_symbol_keys(transaction, source_scope)? {
            symbols_by_name
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol);
        }
        symbols_by_name
    } else {
        BTreeMap::new()
    };
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE source_scope = ?1 AND document_kind = 'import'
        ",
        params![source_scope],
    )?;
    let mut update_import = transaction.prepare(
        "
        UPDATE code_repository_imports
        SET target_hint = ?3,
            resolution_state = ?4,
            confidence_basis_points = ?5,
            confidence_tier = ?6
        WHERE source_scope = ?1 AND import_id = ?2
        ",
    )?;
    let mut search_documents = super::super::SearchDocumentInserter::new(transaction)?;
    for import in imports {
        let language = files.get(&import.path).map(String::as_str);
        let resolution = match language {
            Some("c" | "cpp") => {
                resolve_include_import(&import.path, &import.module, &module_paths)
            }
            Some("python") => resolve_python_import(
                &import.path,
                &import.module,
                &module_paths,
                &symbols_by_name,
            ),
            Some("go") => go_imports::resolve_import(&import.module, &module_paths),
            Some("java") => resolve_java_import(&import.module, &module_paths, &symbols_by_name),
            _ => ImportResolution::Unresolved,
        };
        let (state, confidence, tier, target_hint) =
            import_resolution_fields(resolution, &import.module);
        update_import.execute(params![
            source_scope,
            import.import_id,
            target_hint,
            state,
            confidence,
            tier
        ])?;
        search_documents.insert(
            source_scope,
            "import",
            &import.import_id,
            &import.path,
            language.unwrap_or_default(),
            [
                import.module.as_str(),
                target_hint.as_str(),
                import.path.as_str(),
            ],
        )?;
    }

    Ok(())
}

fn rebuild_calls(
    transaction: &Transaction<'_>,
    source_scope: &str,
    repository_id: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM code_repository_calls WHERE source_scope = ?1",
        params![source_scope],
    )?;
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE source_scope = ?1 AND document_kind = 'call'
        ",
        params![source_scope],
    )?;
    let symbols = load_symbol_keys(transaction, source_scope)?;
    let mut by_path = BTreeMap::<String, Vec<SymbolKey>>::new();
    for symbol in &symbols {
        by_path
            .entry(symbol.path.clone())
            .or_default()
            .push(symbol.clone());
    }
    let mut insert_call = transaction.prepare(
        "
        INSERT INTO code_repository_calls (
            repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id,
            caller_name, callee_symbol_snapshot_id, callee_name, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
    )?;
    for reference in load_call_references(transaction, source_scope)? {
        let caller = caller_for_line(by_path.get(&reference.path), reference.line_start);
        let call_id = stable_id(
            "call",
            [
                repository_id,
                source_scope,
                reference.reference_id.as_str(),
                reference.path.as_str(),
                reference.name.as_str(),
                &reference.line_start.to_string(),
            ],
        );
        insert_call.execute(params![
            repository_id,
            source_scope,
            call_id,
            reference.file_id,
            reference.path,
            caller.map(|symbol| symbol.symbol_snapshot_id.clone()),
            caller.map(|symbol| symbol.name.clone()),
            reference.target_symbol_snapshot_id,
            reference.name,
            reference.target_hint,
            reference.resolution_state,
            reference.confidence_basis_points,
            reference.confidence_tier,
            reference.line_start,
            reference.line_end,
        ])?;
    }
    search_documents::rebuild_call_search_documents(transaction, source_scope)?;

    Ok(())
}

#[derive(Debug, Clone)]
struct SymbolKey {
    symbol_snapshot_id: String,
    path: String,
    name: String,
    line_range: RepositoryCodeRange,
}

#[derive(Debug)]
struct ReferenceKey {
    reference_id: String,
    file_id: String,
    path: String,
    name: String,
    line_start: u32,
    line_end: u32,
    target_symbol_snapshot_id: Option<String>,
    target_hint: Option<String>,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
}

#[derive(Debug)]
struct ImportKey {
    import_id: String,
    path: String,
    module: String,
}

fn caller_for_line(symbols: Option<&Vec<SymbolKey>>, line: u32) -> Option<&SymbolKey> {
    let symbols = symbols?;
    let candidate_end = symbols.partition_point(|symbol| symbol.line_range.start <= line);
    symbols[..candidate_end]
        .iter()
        .rev()
        .find(|symbol| symbol.line_range.end >= line)
}

fn load_symbol_keys(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<SymbolKey>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT symbol_snapshot_id, path, name, line_start, line_end
        FROM code_repository_symbols
        WHERE source_scope = ?1
        ORDER BY path ASC, line_start ASC, line_end DESC, name ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(SymbolKey {
            symbol_snapshot_id: row.get(0)?,
            path: row.get(1)?,
            name: row.get(2)?,
            line_range: RepositoryCodeRange {
                start: row.get(3)?,
                end: row.get(4)?,
            },
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn load_call_references(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<ReferenceKey>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT reference_id, file_id, path, name, line_start, line_end,
               target_symbol_snapshot_id, target_hint, resolution_state,
               confidence_basis_points, confidence_tier
        FROM code_repository_references
        WHERE source_scope = ?1 AND kind = 'call'
        ",
    )?;
    let rows = statement.query_map(params![source_scope], reference_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn reference_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReferenceKey> {
    Ok(ReferenceKey {
        reference_id: row.get(0)?,
        file_id: row.get(1)?,
        path: row.get(2)?,
        name: row.get(3)?,
        line_start: row.get(4)?,
        line_end: row.get(5)?,
        target_symbol_snapshot_id: row.get(6)?,
        target_hint: row.get(7)?,
        resolution_state: row.get(8)?,
        confidence_basis_points: row.get(9)?,
        confidence_tier: row.get(10)?,
    })
}

fn load_import_keys(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<ImportKey>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT import_id, path, module
        FROM code_repository_imports
        WHERE source_scope = ?1
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(ImportKey {
            import_id: row.get(0)?,
            path: row.get(1)?,
            module: row.get(2)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn load_file_languages(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<BTreeMap<String, String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT path, language_id
        FROM code_repository_files
        WHERE source_scope = ?1
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let pairs = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(pairs.into_iter().collect())
}

#[derive(Clone)]
enum ImportResolution {
    Resolved(String),
    Ambiguous,
    Unresolved,
}

fn resolve_include_import(
    import_path: &str,
    statement: &str,
    module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    let Some((target, quoted)) = include_target(statement) else {
        return ImportResolution::Unresolved;
    };
    let mut candidates = Vec::new();
    if quoted {
        if let Some(relative) = normalize_join(parent_dir(import_path), target) {
            push_candidate(&mut candidates, relative);
        }
    }
    push_candidate(&mut candidates, target.to_owned());
    if !target.starts_with("include/") {
        push_candidate(&mut candidates, format!("include/{target}"));
    }

    resolve_first_module_file(&candidates, quoted, module_paths)
}

fn resolve_python_import(
    import_path: &str,
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    if !(import_path.ends_with(".py") || import_path.ends_with(".pyw")) {
        return ImportResolution::Unresolved;
    }
    let statement = statement.trim().trim_end_matches(';').trim();
    if let Some(body) = statement.strip_prefix("from ") {
        let Some((module, names)) = body.split_once(" import ") else {
            return ImportResolution::Unresolved;
        };
        let module_paths = python_module_path_candidates(import_path, module.trim());
        if module_paths.is_empty() {
            return ImportResolution::Unresolved;
        }
        let imported_names = parse_python_imported_names(names);
        return combined_python_import_resolution(
            imported_names.iter().map(|name| {
                resolve_python_imported_name(
                    name,
                    module_paths.as_slice(),
                    indexed_module_paths,
                    symbols_by_name,
                )
            }),
            statement,
        );
    }
    if let Some(body) = statement.strip_prefix("import ") {
        let resolved = body
            .split(',')
            .filter_map(|part| {
                let module = part
                    .trim()
                    .split_once(" as ")
                    .map_or(part.trim(), |(module, _)| module.trim());
                absolute_python_module_path(module)
            })
            .any(|module_path| python_module_exists(&module_path, indexed_module_paths));
        return if resolved {
            ImportResolution::Resolved(statement.to_owned())
        } else {
            ImportResolution::Unresolved
        };
    }

    ImportResolution::Unresolved
}

fn resolve_java_import(
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    match JavaImportRequest::parse(statement) {
        Some(JavaImportRequest::Class { class_path }) => {
            resolve_module_file(&java_source_path(&class_path), true, indexed_module_paths)
        }
        Some(JavaImportRequest::PackageWildcard { package_path }) => {
            if directory_has_java_files(&package_path, indexed_module_paths) {
                ImportResolution::Resolved(package_path)
            } else {
                ImportResolution::Unresolved
            }
        }
        Some(JavaImportRequest::StaticMember { class_path, member }) => {
            resolve_symbol_name_in_paths(&member, &[java_source_path(&class_path)], symbols_by_name)
        }
        Some(JavaImportRequest::StaticWildcard { class_path }) => {
            resolve_module_file(&java_source_path(&class_path), true, indexed_module_paths)
        }
        None => ImportResolution::Unresolved,
    }
}

enum JavaImportRequest {
    Class { class_path: String },
    PackageWildcard { package_path: String },
    StaticMember { class_path: String, member: String },
    StaticWildcard { class_path: String },
}

impl JavaImportRequest {
    fn parse(statement: &str) -> Option<Self> {
        let body = statement
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("import ")?;
        let (is_static, body) = body
            .strip_prefix("static ")
            .map_or((false, body), |body| (true, body.trim()));
        if body.is_empty() {
            return None;
        }
        if let Some(prefix) = body.strip_suffix(".*") {
            let path = prefix.replace('.', "/");
            return if is_static {
                Some(Self::StaticWildcard { class_path: path })
            } else {
                Some(Self::PackageWildcard { package_path: path })
            };
        }

        let (parent, name) = body.rsplit_once('.')?;
        let parent_path = parent.replace('.', "/");
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if is_static {
            Some(Self::StaticMember {
                class_path: parent_path,
                member: name.to_owned(),
            })
        } else {
            Some(Self::Class {
                class_path: format!("{parent_path}/{name}"),
            })
        }
    }
}

fn java_source_path(class_path: &str) -> String {
    format!("{class_path}.java")
}

fn directory_has_java_files(
    directory_path: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
) -> bool {
    let directory = normalize_module_path(directory_path);
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    indexed_module_paths
        .range(prefix.clone()..)
        .take_while(|(path, _)| prefix.is_empty() || path.starts_with(&prefix))
        .any(|(path, _)| path.ends_with(".java"))
}

fn resolve_python_imported_name(
    name: &str,
    module_paths: &[String],
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    let symbol_paths = module_paths
        .iter()
        .flat_map(|module_path| python_module_files(module_path))
        .collect::<Vec<_>>();
    match resolve_symbol_name_in_paths(name, &symbol_paths, symbols_by_name) {
        ImportResolution::Unresolved => {
            if module_paths.iter().any(|module_path| {
                python_module_exists(&format!("{module_path}/{name}"), indexed_module_paths)
            }) {
                ImportResolution::Resolved(name.to_owned())
            } else {
                ImportResolution::Unresolved
            }
        }
        resolution => resolution,
    }
}

fn resolve_symbol_name_in_paths(
    name: &str,
    symbol_paths: &[String],
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    let matching_symbols = symbols_by_name.get(name).map_or(0, |symbols| {
        symbols
            .iter()
            .filter(|symbol| {
                symbol_paths
                    .iter()
                    .any(|module_path| path_matches_candidate(&symbol.path, module_path))
            })
            .take(2)
            .count()
    });
    match matching_symbols {
        1 => ImportResolution::Resolved(name.to_owned()),
        2.. => ImportResolution::Ambiguous,
        _ => ImportResolution::Unresolved,
    }
}

fn combined_python_import_resolution(
    results: impl IntoIterator<Item = ImportResolution>,
    statement: &str,
) -> ImportResolution {
    let mut total = 0usize;
    let mut resolved = 0usize;
    let mut ambiguous = false;
    for result in results {
        total += 1;
        match result {
            ImportResolution::Resolved(_) => resolved += 1,
            ImportResolution::Ambiguous => ambiguous = true,
            ImportResolution::Unresolved => {}
        }
    }
    if total == 0 {
        return ImportResolution::Unresolved;
    }
    if ambiguous || (resolved > 0 && resolved < total) {
        return ImportResolution::Ambiguous;
    }
    if resolved == total {
        return ImportResolution::Resolved(statement.to_owned());
    }

    ImportResolution::Unresolved
}

fn python_module_exists(
    module_path: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
) -> bool {
    python_module_files(module_path)
        .iter()
        .any(|file_path| indexed_module_paths.contains_key(&normalize_module_path(file_path)))
}

fn python_module_files(module_path: &str) -> Vec<String> {
    vec![
        format!("{module_path}.py"),
        format!("{module_path}.pyw"),
        format!("{module_path}/__init__.py"),
    ]
}

fn absolute_python_module_path(module: &str) -> Option<String> {
    let module = module.trim();
    (!module.is_empty() && !module.starts_with('.')).then(|| module.replace('.', "/"))
}

fn python_module_path_candidates(import_path: &str, module: &str) -> Vec<String> {
    let module = module.trim();
    if module.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    if module.starts_with('.') {
        if let Some(relative) = relative_python_module_path(import_path, module) {
            candidates.push(relative);
        }
    } else if let Some(absolute) = absolute_python_module_path(module) {
        candidates.push(absolute);
    }
    candidates.sort();
    candidates.dedup();

    candidates
}

fn relative_python_module_path(import_path: &str, module: &str) -> Option<String> {
    let dot_count = module
        .chars()
        .take_while(|character| *character == '.')
        .count();
    let remainder = module[dot_count..].replace('.', "/");
    let mut package = parent_dir(strip_source_root(import_path))
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let drop_count = dot_count.saturating_sub(1);
    if drop_count > package.len() {
        return None;
    }
    for _ in 0..drop_count {
        package.pop();
    }
    let base = package.join("/");
    if remainder.is_empty() {
        return (!base.is_empty()).then_some(base);
    }

    normalize_join(&base, &remainder)
}

fn parse_python_imported_names(names: &str) -> Vec<String> {
    names
        .replace(['(', ')', '\\'], " ")
        .split(',')
        .filter_map(|part| {
            let name = part
                .trim()
                .split_once(" as ")
                .map_or(part.trim(), |(name, _)| name.trim());
            let name = name.trim_start_matches('.');
            (!name.is_empty() && name != "*").then(|| name.to_owned())
        })
        .collect()
}

fn path_matches_candidate(path: &str, candidate: &str) -> bool {
    let candidate = normalize_module_path(candidate);
    path == candidate || strip_source_root(path) == candidate
}

fn resolve_first_module_file(
    candidates: &[String],
    allow_source_root_match: bool,
    module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    for candidate in candidates {
        match resolve_module_file(candidate, allow_source_root_match, module_paths) {
            ImportResolution::Resolved(path) => return ImportResolution::Resolved(path),
            ImportResolution::Ambiguous => return ImportResolution::Ambiguous,
            ImportResolution::Unresolved => {}
        }
    }

    ImportResolution::Unresolved
}

fn resolve_module_file(
    module_path: &str,
    allow_source_root_match: bool,
    module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    let key = normalize_module_path(module_path);
    let Some(files) = module_paths.get(&key) else {
        return ImportResolution::Unresolved;
    };
    let exact = files
        .iter()
        .filter(|path| path.as_str() == module_path)
        .take(2)
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return ImportResolution::Resolved(exact[0].to_string());
    }
    if !allow_source_root_match {
        return ImportResolution::Unresolved;
    }
    let source_root = files
        .iter()
        .filter(|path| strip_source_root(path) == module_path)
        .take(2)
        .collect::<Vec<_>>();
    if source_root.len() == 1 {
        return ImportResolution::Resolved(source_root[0].to_string());
    }
    if files.len() == 1 {
        return ImportResolution::Resolved(files[0].clone());
    }

    ImportResolution::Ambiguous
}

fn import_resolution_fields(
    resolution: ImportResolution,
    module: &str,
) -> (&'static str, u16, &'static str, String) {
    match resolution {
        ImportResolution::Resolved(target_hint) => ("resolved", 8_000, "inferred", target_hint),
        ImportResolution::Ambiguous => ("ambiguous", 5_000, "ambiguous", module.to_owned()),
        ImportResolution::Unresolved => ("unresolved", 2_500, "ambiguous", module.to_owned()),
    }
}

fn include_target(statement: &str) -> Option<(&str, bool)> {
    let statement = statement.trim();
    if !statement.starts_with("#include") {
        return None;
    }
    if let Some(target) = parse_quoted_specifier(statement) {
        return Some((target, true));
    }
    let start = statement.find('<')?;
    let rest = &statement[start + 1..];
    let end = rest.find('>')?;

    Some((&rest[..end], false))
}

fn parse_quoted_specifier(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\''])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
}

fn module_path_index<'a>(
    paths: impl IntoIterator<Item = &'a String>,
) -> BTreeMap<String, Vec<String>> {
    let mut module_paths = BTreeMap::<String, Vec<String>>::new();
    for path in paths {
        module_paths
            .entry(strip_source_root(path).to_owned())
            .or_default()
            .push(path.clone());
    }

    module_paths
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn normalize_join(parent: &str, child: &str) -> Option<String> {
    let mut parts = Vec::<&str>::new();
    if child.starts_with('/') {
        return None;
    }
    for part in parent
        .split('/')
        .chain(child.split('/'))
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if part == ".." {
            parts.pop()?;
        } else {
            parts.push(part);
        }
    }
    if parts.is_empty() {
        return None;
    }

    Some(parts.join("/"))
}

fn normalize_module_path(path: &str) -> String {
    strip_source_root(path.trim_start_matches("./")).to_owned()
}

fn strip_source_root(path: &str) -> &str {
    for prefix in [
        "src/main/java/",
        "src/test/java/",
        "src/main/kotlin/",
        "src/test/kotlin/",
        "src/main/scala/",
        "src/test/scala/",
        "src/main/groovy/",
        "src/test/groovy/",
        "staging/src/",
        "vendor/",
        "src/",
    ] {
        if let Some(stripped) = path.strip_prefix(prefix) {
            return stripped;
        }
    }

    path
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn stable_id<'a>(prefix: &str, parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }

    format!("{prefix}:{:016x}", stable_hash64(&bytes))
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::{SymbolKey, caller_for_line};
    use crate::domain::RepositoryCodeRange;

    #[test]
    fn caller_lookup_uses_sorted_prefix_and_prefers_innermost_symbol() {
        let symbols = vec![
            symbol("outer", 10, 100),
            symbol("same_start_outer", 20, 80),
            symbol("same_start_inner", 20, 40),
            symbol("after_call", 60, 70),
        ];

        let caller = caller_for_line(Some(&symbols), 30).expect("caller should match");

        assert_eq!(caller.name, "same_start_inner");
    }

    #[test]
    fn caller_lookup_ignores_symbols_that_start_after_call_line() {
        let symbols = vec![symbol("before", 1, 5), symbol("after", 20, 30)];

        assert!(caller_for_line(Some(&symbols), 10).is_none());
    }

    fn symbol(name: &str, start: u32, end: u32) -> SymbolKey {
        SymbolKey {
            symbol_snapshot_id: format!("symbol:{name}"),
            path: "src/lib.rs".to_owned(),
            name: name.to_owned(),
            line_range: RepositoryCodeRange { start, end },
        }
    }
}
