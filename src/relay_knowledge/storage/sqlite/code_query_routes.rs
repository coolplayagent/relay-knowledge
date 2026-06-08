use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest,
        RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts,
    code_query_support::{
        CandidateLayer, ScoreQuery, candidate_limit, fts_match_query,
        fts_values_for_limited_with_language, language_filter_sql_for_column,
        path_filter_sql_for_column, score_exact_path,
    },
    hit_from_parts, prepare_code_search_statement, required_scope, selected_row,
};

struct RouteRow {
    file_id: String,
    path: String,
    language_id: String,
    url: String,
    http_method: String,
    handler_name: String,
    handler_symbol_snapshot_id: Option<String>,
    framework: String,
    line_range: RepositoryCodeRange,
    handler_canonical_symbol_id: Option<String>,
    parse_status: String,
    degraded_reason: Option<String>,
}

pub(super) fn search_routes(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let source_scope = required_scope(status)?;
    let fts_query = fts_match_query(&request.query);
    let route_limit = candidate_limit(request, CandidateLayer::Chunk);
    let path_filter = path_filter_sql_for_column("path", status, request);
    let language_filter = language_filter_sql_for_column("language_id", status, request);
    let sql = format!(
        "
        SELECT route.file_id, route.path, route.language_id, route.url, route.http_method,
               route.handler_name, route.handler_symbol_snapshot_id, route.framework,
               route.line_start, route.line_end, symbol.canonical_symbol_id,
               file.parse_status, file.degraded_reason
        FROM code_repository_routes route
        INNER JOIN code_repository_files file
            ON file.source_scope = route.source_scope AND file.path = route.path
        LEFT JOIN code_repository_symbols symbol
            ON symbol.source_scope = route.source_scope
           AND symbol.symbol_snapshot_id = route.handler_symbol_snapshot_id
        WHERE route.source_scope = ?
          AND route.route_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'route'
                {path_filter}
                {language_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        ORDER BY route.path ASC, route.line_start ASC, route.url ASC, route.http_method ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(route_fts_values(
            source_scope,
            status,
            request,
            &fts_query,
            route_limit,
        )),
        |row| {
            Ok(RouteRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                url: row.get(3)?,
                http_method: row.get(4)?,
                handler_name: row.get(5)?,
                handler_symbol_snapshot_id: row.get(6)?,
                framework: row.get(7)?,
                line_range: RepositoryCodeRange {
                    start: row.get(8)?,
                    end: row.get(9)?,
                },
                handler_canonical_symbol_id: row.get(10)?,
                parse_status: row.get(11)?,
                degraded_reason: row.get(12)?,
            })
        },
    )?;
    let score_query = ScoreQuery::new(&request.query);
    let query = request.query.to_ascii_lowercase();
    let mut hits = Vec::new();
    for row in rows {
        let row = row.map_err(StorageError::from)?;
        if !selected_row(&row.path, &row.language_id, status, request) {
            continue;
        }
        let score = score_query.score([
            row.url.as_str(),
            row.http_method.as_str(),
            row.handler_name.as_str(),
            row.framework.as_str(),
            row.path.as_str(),
        ]) + score_exact_path(&query, &row.path)
            + exact_route_url_bonus(&query, &row.url);
        let edge_resolution_state = if row.handler_symbol_snapshot_id.is_some() {
            "resolved"
        } else {
            "unresolved"
        };
        hits.push(hit_from_parts(
            status,
            HitParts {
                path: row.path,
                language_id: row.language_id,
                byte_range: RepositoryCodeRange { start: 0, end: 0 },
                line_range: row.line_range,
                symbol_snapshot_id: row.handler_symbol_snapshot_id,
                canonical_symbol_id: row.handler_canonical_symbol_id,
                file_id: Some(row.file_id),
                retrieval_layers: route_layers(&row.parse_status),
                score,
                excerpt: format!(
                    "{} {} -> {} ({})",
                    row.http_method.to_ascii_uppercase(),
                    row.url,
                    row.handler_name,
                    row.framework
                ),
                degraded_reason: row.degraded_reason,
                edge_kind: Some("route".to_owned()),
                edge_resolution_state: Some(edge_resolution_state.to_owned()),
                edge_target_hint: Some(row.handler_name),
                edge_confidence_basis_points: Some(10_000),
                edge_confidence_tier: Some("extracted".to_owned()),
            },
        ));
    }

    Ok(hits)
}

fn route_fts_values(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    fts_query: &str,
    limit: usize,
) -> Vec<Value> {
    fts_values_for_limited_with_language(source_scope, status, request, fts_query, limit, limit)
}

fn exact_route_url_bonus(query: &str, url: &str) -> f64 {
    if query.trim() == url.to_ascii_lowercase() {
        6.0
    } else {
        0.0
    }
}

fn route_layers(parse_status: &str) -> Vec<CodeRetrievalLayer> {
    let mut layers = vec![CodeRetrievalLayer::Lexical];
    if parse_status != "parsed" {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}
