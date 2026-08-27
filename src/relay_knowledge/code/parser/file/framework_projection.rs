//! Framework graph projection from one already parsed source file.

use crate::code::{CodeIndexError, SnapshotBuild};

use super::super::frameworks::{self, FrameworkFileInput};

pub(super) fn record_framework_graph(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    symbols: &[crate::domain::RepositoryCodeSymbolRecord],
) -> Result<(), CodeIndexError> {
    let facts = frameworks::extract(
        build,
        FrameworkFileInput {
            path,
            file_id,
            language_id,
            content,
            symbols,
        },
    )?;
    build.framework_nodes.extend(facts.nodes);
    build.framework_edges.extend(facts.edges);
    Ok(())
}

#[cfg(test)]
#[path = "framework_projection_tests.rs"]
mod tests;
