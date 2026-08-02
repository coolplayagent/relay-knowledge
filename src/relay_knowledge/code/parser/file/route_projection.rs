//! Route record materialization and handler-symbol association.

use std::collections::BTreeMap;

use crate::{
    code::{SnapshotBuild, stable_id},
    domain::{CodeRouteRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord, SymbolRole},
};

use crate::code::parser::routes::{ANONYMOUS_ROUTE_HANDLER_NAME, detect_routes};

pub(super) fn record_routes(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
) {
    let _span = tracing::debug_span!("record_routes", path, file_id, language_id).entered();
    let candidates = detect_routes(language_id, content);
    if candidates.is_empty() {
        return;
    }
    tracing::debug!(route_count = candidates.len(), "detected web routes");
    let symbol_index = route_handler_symbol_index(&build.symbols);
    for candidate in candidates {
        let route_id = stable_id(
            "route",
            [
                &build.repository_id,
                &build.source_scope,
                path,
                &candidate.url,
                &candidate.http_method,
                &candidate.handler_name,
                &candidate.line.to_string(),
            ],
        );
        let line_range =
            match RepositoryCodeRange::new("line_range", candidate.line, candidate.line) {
                Ok(range) => range,
                Err(error) => {
                    tracing::debug!(
                        path,
                        route_line = candidate.line,
                        error = %error,
                        "skipping route with invalid source range"
                    );
                    continue;
                }
            };
        let route_idx = build.routes.len();
        build.routes.push(CodeRouteRecord {
            repository_id: build.repository_id.clone(),
            source_scope: build.source_scope.clone(),
            route_id,
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            language_id: language_id.to_owned(),
            url: candidate.url.clone(),
            http_method: candidate.http_method.clone(),
            handler_name: candidate.handler_name.clone(),
            handler_symbol_snapshot_id: None,
            framework: candidate.framework,
            line_range,
        });
        if candidate.handler_name != ANONYMOUS_ROUTE_HANDLER_NAME {
            annotate_route_handler_symbol(
                build,
                &symbol_index,
                RouteHandlerAnnotation {
                    route_idx,
                    path,
                    handler_name: &candidate.handler_name,
                    url: &candidate.url,
                    http_method: &candidate.http_method,
                    route_line: candidate.line,
                },
            );
        }
    }
}

fn route_handler_symbol_index(
    symbols: &[RepositoryCodeSymbolRecord],
) -> BTreeMap<(String, String), Vec<usize>> {
    let mut index = BTreeMap::<(String, String), Vec<usize>>::new();
    for (symbol_idx, symbol) in symbols.iter().enumerate() {
        index
            .entry((symbol.path.clone(), symbol.name.clone()))
            .or_default()
            .push(symbol_idx);
    }

    index
}

struct RouteHandlerAnnotation<'a> {
    route_idx: usize,
    path: &'a str,
    handler_name: &'a str,
    url: &'a str,
    http_method: &'a str,
    route_line: usize,
}

fn annotate_route_handler_symbol(
    build: &mut SnapshotBuild,
    symbol_index: &BTreeMap<(String, String), Vec<usize>>,
    annotation: RouteHandlerAnnotation<'_>,
) {
    let route_line = annotation.route_line as u32;
    let Some((symbol_name, symbol_indices)) =
        route_handler_symbol_candidates(symbol_index, annotation.path, annotation.handler_name)
    else {
        tracing::debug!(
            path = annotation.path,
            handler_name = annotation.handler_name,
            "route handler symbol was not found"
        );
        return;
    };
    let symbol_idx = symbol_indices
        .iter()
        .copied()
        .filter(|idx| {
            build.symbols[*idx].path == annotation.path && build.symbols[*idx].name == symbol_name
        })
        .min_by_key(|idx| build.symbols[*idx].line_range.start.abs_diff(route_line));

    if let Some(symbol_idx) = symbol_idx {
        let sym = &mut build.symbols[symbol_idx];
        let symbol_snapshot_id = sym.symbol_snapshot_id.clone();
        let url = annotation.url.to_owned();
        let http_method = annotation.http_method.to_owned();
        if let Some(role) = &mut sym.symbol_role {
            role.merge_route_handler(url, http_method);
        } else {
            sym.symbol_role = Some(SymbolRole::RouteHandler { url, http_method });
        }
        if let Some(route) = build.routes.get_mut(annotation.route_idx) {
            route.handler_symbol_snapshot_id = Some(symbol_snapshot_id);
        }
    } else {
        tracing::debug!(
            path = annotation.path,
            handler_name = annotation.handler_name,
            route_line,
            "route handler symbol was not linked"
        );
    }
}

fn route_handler_symbol_candidates<'a>(
    symbol_index: &'a BTreeMap<(String, String), Vec<usize>>,
    path: &str,
    handler_name: &str,
) -> Option<(String, &'a Vec<usize>)> {
    if let Some(symbol_indices) = symbol_index.get(&(path.to_owned(), handler_name.to_owned())) {
        return Some((handler_name.to_owned(), symbol_indices));
    }
    let leaf_name = handler_name.rsplit('.').next()?;
    if leaf_name == handler_name {
        return None;
    }
    symbol_index
        .get(&(path.to_owned(), leaf_name.to_owned()))
        .map(|symbol_indices| (leaf_name.to_owned(), symbol_indices))
}

#[cfg(test)]
#[path = "route_projection_tests.rs"]
mod tests;
