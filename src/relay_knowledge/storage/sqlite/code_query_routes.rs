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
        CandidateLayer, ScoreQuery, candidate_limit, escape_sql_like, fts_match_query,
        fts_path_and_language_filter_sql, fts_values_for_limited_with_language,
        language_filter_sql_for_columns, path_filter_sql_for_column, push_language_filter_values,
        push_path_filter_values, score_exact_path,
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
    is_generated: bool,
    degraded_reason: Option<String>,
}

pub(super) fn search_routes(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let source_scope = required_scope(status)?;
    let route_query = RouteQuery::new(&request.query);
    let route_limit = candidate_limit(request, CandidateLayer::Chunk);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let route_fallback_filter = route_fallback_filter_sql(status, request);
    let route_fallback_method_filter = route_fallback_method_filter_sql(&route_query);
    let sql = format!(
        "
        SELECT route.file_id, route.path, route.language_id, route.url, route.http_method,
               route.handler_name, route.handler_symbol_snapshot_id, route.framework,
               route.line_start, route.line_end, symbol.canonical_symbol_id,
               file.parse_status, file.is_generated, file.degraded_reason
        FROM code_repository_routes route
        INNER JOIN code_repository_files file
            ON file.source_scope = route.source_scope AND file.path = route.path
        LEFT JOIN code_repository_symbols symbol
            ON symbol.source_scope = route.source_scope
           AND symbol.symbol_snapshot_id = route.handler_symbol_snapshot_id
        WHERE route.source_scope = ?
          AND (
              route.route_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'route'
                {fts_filter}
              ORDER BY coalesce((SELECT fts_file.is_generated FROM code_repository_files fts_file WHERE fts_file.source_scope = code_repository_search.source_scope AND fts_file.path = code_repository_search.path LIMIT 1), 0) ASC,
                  bm25(code_repository_search) ASC,
                  record_id ASC
              LIMIT ?
              )
              OR (
                  ? != ''
                  AND route.url LIKE ? ESCAPE '\\'
                  {route_fallback_filter}
                  {route_fallback_method_filter}
              )
          )
        ORDER BY file.is_generated ASC, route.path ASC, route.line_start ASC, route.url ASC, route.http_method ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(route_fts_values(
            source_scope,
            status,
            request,
            &route_query.fts_query,
            route_limit,
            &route_query,
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
                is_generated: row.get::<_, i64>(12)? != 0,
                degraded_reason: row.get(13)?,
            })
        },
    )?;
    let score_query = ScoreQuery::new(&request.query);
    let query = request.query.to_ascii_lowercase();
    let mut hits = Vec::new();
    for row in rows {
        let row = row.map_err(StorageError::from)?;
        if !selected_row(
            &row.path,
            &row.language_id,
            row.is_generated,
            status,
            request,
        ) {
            continue;
        }
        let score = score_query.score([
            row.url.as_str(),
            row.http_method.as_str(),
            row.handler_name.as_str(),
            row.framework.as_str(),
            row.path.as_str(),
        ]) + score_exact_path(&query, &row.path)
            + route_query.exact_url_bonus(&row.url)
            + route_query.parameterized_url_bonus(&row.url);
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
                is_generated: row.is_generated,
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

fn route_fallback_filter_sql(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut filter = String::new();
    filter.push_str(&path_filter_sql_for_column("route.path", status, request));
    filter.push_str(&language_filter_sql_for_columns(
        "route.language_id",
        "route.path",
        status,
        request,
    ));
    if request.exclude_generated {
        filter.push_str(" AND file.is_generated = 0");
    }

    filter
}

fn route_fallback_method_filter_sql(route_query: &RouteQuery) -> &'static str {
    if route_query.http_method.is_some() {
        " AND (route.http_method = ? OR route.http_method = 'any')"
    } else {
        ""
    }
}

fn route_fts_values(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    fts_query: &str,
    limit: usize,
    route_query: &RouteQuery,
) -> Vec<Value> {
    let mut values = fts_values_for_limited_with_language(
        source_scope,
        status,
        request,
        fts_query,
        limit,
        limit,
    );
    let final_limit = values.pop().unwrap_or(Value::Integer(limit as i64));
    let fallback_like = route_query.fallback_like.as_deref().unwrap_or("");
    values.push(Value::Text(fallback_like.to_owned()));
    values.push(Value::Text(fallback_like.to_owned()));
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    if let Some(http_method) = &route_query.http_method {
        values.push(Value::Text(http_method.clone()));
    }
    values.push(final_limit);
    values
}

struct RouteQuery {
    fts_query: String,
    url: Option<String>,
    http_method: Option<String>,
    fallback_like: Option<String>,
}

impl RouteQuery {
    fn new(query: &str) -> Self {
        let url = route_query_url(query);
        Self {
            fts_query: fts_match_query(&route_query_fts_text(query)),
            fallback_like: url.as_deref().and_then(route_url_fallback_like),
            http_method: route_query_http_method(query),
            url,
        }
    }

    fn exact_url_bonus(&self, url: &str) -> f64 {
        if self
            .url
            .as_deref()
            .is_some_and(|query_url| query_url == url.to_ascii_lowercase())
        {
            6.0
        } else {
            0.0
        }
    }

    fn parameterized_url_bonus(&self, route_url: &str) -> f64 {
        if self
            .url
            .as_deref()
            .is_some_and(|query_url| route_url_matches_parameterized_query(route_url, query_url))
        {
            5.0
        } else {
            0.0
        }
    }
}

fn route_query_http_method(query: &str) -> Option<String> {
    query.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|character: char| !character.is_ascii_alphabetic());
        let method = token.to_ascii_lowercase();
        matches!(
            method.as_str(),
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
        )
        .then_some(method)
    })
}

fn route_query_fts_text(query: &str) -> String {
    let mut terms = Vec::new();
    for token in query.split_whitespace() {
        if let Some(url) = normalized_route_url_token(token) {
            terms.extend(route_url_fts_segments(&url));
        } else {
            terms.push(token.to_owned());
        }
    }

    if terms.is_empty() {
        query.to_owned()
    } else {
        terms.join(" ")
    }
}

fn route_query_url(query: &str) -> Option<String> {
    query
        .split_whitespace()
        .find_map(normalized_route_url_token)
}

fn normalized_route_url_token(token: &str) -> Option<String> {
    let token = token.trim_matches(|character: char| {
        matches!(
            character,
            '`' | '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    });
    if !token.starts_with('/') {
        return None;
    }
    let end = token.find(['?', '#']).unwrap_or(token.len());
    let path = &token[..end];
    (path.len() > 1).then(|| path.to_ascii_lowercase())
}

fn route_url_fts_segments(url: &str) -> Vec<String> {
    route_url_segments(url)
        .into_iter()
        .filter(|segment| !concrete_route_query_segment(segment))
        .map(str::to_owned)
        .collect()
}

fn route_url_fallback_like(url: &str) -> Option<String> {
    let segments = route_url_segments(url);
    if segments.len() < 2 {
        return None;
    }
    let prefix = format!("/{}", segments[..segments.len() - 1].join("/"));
    Some(format!("{}/%", escape_sql_like(&prefix)))
}

fn route_url_matches_parameterized_query(route_url: &str, query_url: &str) -> bool {
    let route_segments = route_url_segments(route_url);
    let query_segments = route_url_segments(query_url);
    if route_segments.len() != query_segments.len() {
        return false;
    }
    let mut matched_parameter = false;
    for (route_segment, query_segment) in route_segments.iter().zip(query_segments.iter()) {
        if route_parameter_segment(route_segment) {
            matched_parameter = true;
            continue;
        }
        if !route_segment.eq_ignore_ascii_case(query_segment) {
            return false;
        }
    }
    matched_parameter
}

fn route_url_segments(url: &str) -> Vec<&str> {
    url.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn route_parameter_segment(segment: &str) -> bool {
    segment.starts_with(':')
        || (segment.starts_with('{') && segment.ends_with('}'))
        || (segment.starts_with('<') && segment.ends_with('>'))
}

fn concrete_route_query_segment(segment: &str) -> bool {
    segment.chars().all(|character| character.is_ascii_digit())
        || (segment.len() >= 8
            && segment
                .chars()
                .all(|character| character.is_ascii_hexdigit() || character == '-'))
}

fn route_layers(parse_status: &str) -> Vec<CodeRetrievalLayer> {
    let mut layers = vec![CodeRetrievalLayer::Lexical];
    if parse_status != "parsed" {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}
