use rusqlite::Connection;

use crate::storage::{
    GraphCanvasSelection, GraphCanvasStorageRequest, GraphCanvasStorageSnapshot, StorageError,
};

mod code;
mod context;
mod facts;
mod knowledge;
mod nodes;

use context::{CanvasBuilder, CanvasFilter};

const MAX_CANVAS_LIMIT: usize = 1000;

pub(super) fn graph_canvas(
    connection: &mut Connection,
    request: GraphCanvasStorageRequest,
) -> Result<GraphCanvasStorageSnapshot, StorageError> {
    validate_limit(request.limit)?;
    let mut builder = CanvasBuilder::new(request.limit);
    let filter = CanvasFilter::new(
        request.source_scope,
        request.query,
        request.graph_version,
        request.limit,
    );

    if request.selection.includes_knowledge() {
        knowledge::add_knowledge_nodes(connection, &mut builder, &filter)?;
        facts::add_structured_facts(connection, &mut builder, &filter)?;
    }
    if request.selection.includes_code() {
        code::add_code_nodes(connection, &mut builder, &filter)?;
    }
    if request.selection == GraphCanvasSelection::Mixed {
        code::add_source_path_links(connection, &mut builder, &filter)?;
    }

    Ok(builder.into_snapshot())
}

fn validate_limit(limit: usize) -> Result<(), StorageError> {
    if limit == 0 {
        return Err(StorageError::InvalidInput(
            "graph canvas limit must be positive".to_owned(),
        ));
    }
    if limit > MAX_CANVAS_LIMIT {
        return Err(StorageError::InvalidInput(format!(
            "graph canvas limit must be at most {MAX_CANVAS_LIMIT}"
        )));
    }

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
