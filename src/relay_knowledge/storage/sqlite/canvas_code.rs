use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{
    domain::GraphVersion,
    storage::{GraphCanvasStorageEdge, GraphCanvasStorageNode, StorageError},
};

use super::canvas::{
    CanvasBuilder, CanvasFilter, code_file_node_id, code_symbol_node_id, collect_rows, detail_map,
    evidence_node_id,
};

pub(super) fn add_code_nodes(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    add_code_files(connection, builder, filter)?;
    add_code_symbols(connection, builder, filter)?;
    add_code_references(connection, builder, filter)?;

    Ok(())
}

fn add_code_files(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, language_id, parse_status, diagnostic, created_graph_version
        FROM code_files
        WHERE created_graph_version <= ?1
          AND (?2 IS NULL OR source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(source_scope || ' ' || path || ' ' || language_id)
              LIKE '%' || lower(?3) || '%'
          )
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.graph_version.get(),
            filter.source_scope.as_deref(),
            filter.query.as_deref(),
            filter.sql_limit()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                GraphVersion::new(row.get::<_, u64>(5)?),
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    builder.observe_query_len(records.len());
    for (scope, path, language, parse_status, diagnostic, graph_version) in
        records.into_iter().take(filter.limit)
    {
        insert_code_file_node(
            builder,
            &scope,
            &path,
            Some(&language),
            Some(&parse_status),
            diagnostic.as_deref(),
            graph_version,
        );
    }

    Ok(())
}

fn add_code_symbols(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, symbol_id, name, kind, start_line, end_line,
               node_kind, capture_kind, created_graph_version
        FROM code_symbols
        WHERE created_graph_version <= ?1
          AND (?2 IS NULL OR source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(source_scope || ' ' || path || ' ' || name || ' ' || kind)
              LIKE '%' || lower(?3) || '%'
          )
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC, start_line ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.graph_version.get(),
            filter.source_scope.as_deref(),
            filter.query.as_deref(),
            filter.sql_limit()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, u32>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                GraphVersion::new(row.get::<_, u64>(9)?),
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    builder.observe_query_len(records.len());
    for (
        scope,
        path,
        symbol_id,
        name,
        kind,
        start_line,
        end_line,
        node_kind,
        capture_kind,
        graph_version,
    ) in records.into_iter().take(filter.limit)
    {
        insert_code_file_node(builder, &scope, &path, None, None, None, graph_version);
        builder.insert_node(GraphCanvasStorageNode {
            id: code_symbol_node_id(&scope, &path, &symbol_id),
            kind: "code_symbol".to_owned(),
            label: name.clone(),
            subtitle: Some(format!("{kind} / {path}:{start_line}")),
            source_scope: Some(scope.clone()),
            graph_version,
            weight: 2,
            status: None,
            details: detail_map([
                ("symbol_id", symbol_id.as_str()),
                ("path", path.as_str()),
                ("symbol_kind", kind.as_str()),
                ("line_range", &format!("{start_line}-{end_line}")),
                ("node_kind", node_kind.as_str()),
                ("capture_kind", capture_kind.as_str()),
            ]),
        });
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("defines:{scope}:{path}:{symbol_id}"),
            kind: "defines".to_owned(),
            source: code_file_node_id(&scope, &path),
            target: code_symbol_node_id(&scope, &path, &symbol_id),
            label: "defines".to_owned(),
            graph_version,
            confidence_basis_points: None,
            evidence_count: None,
            details: BTreeMap::new(),
        });
    }

    Ok(())
}

fn add_code_references(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ref.source_scope, ref.path, ref.reference_id, ref.symbol_text, ref.kind,
               ref.start_line, ref.end_line, ref.resolution_state, ref.target_symbol_id,
               ref.created_graph_version, target.path, target.name
        FROM code_references ref
        LEFT JOIN code_symbols target
          ON target.source_scope = ref.source_scope
         AND target.symbol_id = ref.target_symbol_id
         AND target.created_graph_version <= ?1
         AND (
             target.path = ref.path
             OR NOT EXISTS (
                 SELECT 1
                 FROM code_symbols duplicate
                 WHERE duplicate.source_scope = ref.source_scope
                   AND duplicate.symbol_id = ref.target_symbol_id
                   AND duplicate.created_graph_version <= ?1
                   AND duplicate.path <> target.path
             )
         )
        WHERE ref.created_graph_version <= ?1
          AND (?2 IS NULL OR ref.source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(ref.source_scope || ' ' || ref.path || ' ' ||
              ref.symbol_text || ' ' || ref.kind) LIKE '%' || lower(?3) || '%'
          )
        ORDER BY ref.created_graph_version DESC, ref.source_scope ASC, ref.path ASC, ref.start_line ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.graph_version.get(),
            filter.source_scope.as_deref(),
            filter.query.as_deref(),
            filter.sql_limit()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, u32>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                GraphVersion::new(row.get::<_, u64>(9)?),
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    builder.observe_query_len(records.len());
    for (
        scope,
        path,
        reference_id,
        symbol_text,
        kind,
        start_line,
        end_line,
        resolution_state,
        target_symbol_id,
        graph_version,
        target_path,
        target_name,
    ) in records.into_iter().take(filter.limit)
    {
        insert_code_file_node(builder, &scope, &path, None, None, None, graph_version);
        let target = if let (Some(path), Some(symbol_id)) =
            (target_path.as_ref(), target_symbol_id.as_ref())
        {
            let label = target_name.as_deref().unwrap_or(symbol_text.as_str());
            builder.insert_node(GraphCanvasStorageNode {
                id: code_symbol_node_id(&scope, path, symbol_id),
                kind: "code_symbol".to_owned(),
                label: label.to_owned(),
                subtitle: Some(path.to_owned()),
                source_scope: Some(scope.clone()),
                graph_version,
                weight: 1,
                status: Some(resolution_state.clone()),
                details: detail_map([
                    ("symbol_id", symbol_id.as_str()),
                    ("path", path.as_str()),
                    ("resolution_state", resolution_state.as_str()),
                ]),
            });
            code_symbol_node_id(&scope, path, symbol_id)
        } else {
            let unresolved = format!("symbol-ref:{scope}:{symbol_text}");
            builder.insert_node(GraphCanvasStorageNode {
                id: unresolved.clone(),
                kind: "code_symbol".to_owned(),
                label: symbol_text.clone(),
                subtitle: Some("unresolved reference".to_owned()),
                source_scope: Some(scope.clone()),
                graph_version,
                weight: 1,
                status: Some(resolution_state.clone()),
                details: detail_map([
                    ("symbol_text", symbol_text.as_str()),
                    ("resolution_state", resolution_state.as_str()),
                ]),
            });
            unresolved
        };
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("reference:{scope}:{path}:{reference_id}"),
            kind: kind.clone(),
            source: code_file_node_id(&scope, &path),
            target,
            label: kind,
            graph_version,
            confidence_basis_points: None,
            evidence_count: None,
            details: detail_map([
                ("reference_id", reference_id.as_str()),
                ("symbol_text", symbol_text.as_str()),
                ("line_range", &format!("{start_line}-{end_line}")),
                ("resolution_state", resolution_state.as_str()),
            ]),
        });
    }

    Ok(())
}

pub(super) fn add_source_path_links(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ev.id, ev.source_scope, ev.source_path,
               MAX(ev.created_graph_version, file.created_graph_version)
        FROM evidence ev
        JOIN code_files file ON file.source_scope = ev.source_scope
                            AND file.path = ev.source_path
                            AND file.created_graph_version <= ?1
        WHERE ev.created_graph_version <= ?1
          AND (?2 IS NULL OR ev.source_scope = ?2)
          AND ev.source_path IS NOT NULL
          AND (?3 IS NULL OR lower(ev.source_path || ' ' || ev.content) LIKE '%' || lower(?3) || '%')
        ORDER BY MAX(ev.created_graph_version, file.created_graph_version) DESC, ev.id ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.graph_version.get(),
            filter.source_scope.as_deref(),
            filter.query.as_deref(),
            filter.sql_limit()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                GraphVersion::new(row.get::<_, u64>(3)?),
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    builder.observe_query_len(records.len());
    for (evidence_id, scope, path, graph_version) in records.into_iter().take(filter.limit) {
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("evidence-source-file:{evidence_id}:{scope}:{path}"),
            kind: "source_path".to_owned(),
            source: evidence_node_id(&evidence_id),
            target: code_file_node_id(&scope, &path),
            label: "source".to_owned(),
            graph_version,
            confidence_basis_points: None,
            evidence_count: Some(1),
            details: detail_map([("source_path", path.as_str())]),
        });
    }

    Ok(())
}

fn insert_code_file_node(
    builder: &mut CanvasBuilder,
    scope: &str,
    path: &str,
    language: Option<&str>,
    parse_status: Option<&str>,
    diagnostic: Option<&str>,
    graph_version: GraphVersion,
) {
    let mut details = detail_map([("source_scope", scope), ("path", path)]);
    if let Some(language) = language {
        details.insert("language".to_owned(), language.to_owned());
    }
    if let Some(status) = parse_status {
        details.insert("parse_status".to_owned(), status.to_owned());
    }
    if let Some(diagnostic) = diagnostic {
        details.insert("diagnostic".to_owned(), diagnostic.to_owned());
    }
    builder.insert_scope_node(scope, graph_version);
    builder.insert_node(GraphCanvasStorageNode {
        id: code_file_node_id(scope, path),
        kind: "code_file".to_owned(),
        label: path.to_owned(),
        subtitle: language.map(str::to_owned),
        source_scope: Some(scope.to_owned()),
        graph_version,
        weight: 2,
        status: parse_status.map(str::to_owned),
        details,
    });
    builder.insert_edge(GraphCanvasStorageEdge {
        id: format!("scope-file:{scope}:{path}"),
        kind: "contains".to_owned(),
        source: super::canvas::scope_node_id(scope),
        target: code_file_node_id(scope, path),
        label: "contains".to_owned(),
        graph_version,
        confidence_basis_points: None,
        evidence_count: None,
        details: BTreeMap::new(),
    });
}
