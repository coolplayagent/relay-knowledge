//! Bounded Angular and Vue component/template fact extraction.

mod angular;
mod scan;
mod template;
mod vue;

use crate::{
    code::{CodeIndexError, SnapshotBuild},
    domain::{CodeFrameworkEdgeRecord, CodeFrameworkNodeRecord, RepositoryCodeSymbolRecord},
};

const MAX_FRAMEWORK_NODES_PER_FILE: usize = 512;
const MAX_FRAMEWORK_EDGES_PER_FILE: usize = 2_048;

pub(super) struct FrameworkFileInput<'a> {
    pub(super) path: &'a str,
    pub(super) file_id: &'a str,
    pub(super) language_id: &'a str,
    pub(super) content: &'a str,
    pub(super) symbols: &'a [RepositoryCodeSymbolRecord],
}

#[derive(Default)]
pub(super) struct FrameworkFacts {
    pub(super) nodes: Vec<CodeFrameworkNodeRecord>,
    pub(super) edges: Vec<CodeFrameworkEdgeRecord>,
}

impl FrameworkFacts {
    fn enforce_budget(&self) -> Result<(), CodeIndexError> {
        if self.nodes.len() > MAX_FRAMEWORK_NODES_PER_FILE {
            return Err(CodeIndexError::InvalidInput(format!(
                "framework node budget exceeded: {} > {MAX_FRAMEWORK_NODES_PER_FILE}",
                self.nodes.len()
            )));
        }
        if self.edges.len() > MAX_FRAMEWORK_EDGES_PER_FILE {
            return Err(CodeIndexError::InvalidInput(format!(
                "framework edge budget exceeded: {} > {MAX_FRAMEWORK_EDGES_PER_FILE}",
                self.edges.len()
            )));
        }
        Ok(())
    }
}

pub(super) fn extract(
    build: &SnapshotBuild,
    input: FrameworkFileInput<'_>,
) -> Result<FrameworkFacts, CodeIndexError> {
    let mut facts = FrameworkFacts::default();
    if input.language_id == "vue" {
        vue::extract(build, &input, &mut facts)?;
    } else if input.language_id == "typescript" || input.language_id == "tsx" {
        angular::extract(build, &input, &mut facts)?;
    } else if input.language_id == "html" && angular::looks_like_template(input.path, input.content)
    {
        template::extract_angular_template(build, &input, None, &mut facts)?;
    }
    facts.enforce_budget()?;
    Ok(facts)
}

pub(super) fn vue_script_mask(content: &str) -> Option<(String, bool)> {
    vue::script_mask(content)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
