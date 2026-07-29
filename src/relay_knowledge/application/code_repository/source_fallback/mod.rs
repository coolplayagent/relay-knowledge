mod execution;
mod filters;
mod identity;
mod imports;
mod plan;
mod results;
mod scoring;
mod surface;
mod worktree;

pub(super) use execution::apply_code_grep_fallback;

#[cfg(test)]
use crate::{
    code::{SourceGrepKind, SourceGrepMatch, SourceGrepOutcome},
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
};
#[cfg(test)]
use plan::{CodeGrepFallbackPlan, plan_code_grep_fallback};
#[cfg(test)]
use results::{append_code_grep_fallback, append_definition_source_fallback};
#[cfg(test)]
use scoring::reference_source_grep_score_adjustment;

#[cfg(test)]
#[path = "filter_tests.rs"]
mod filter_tests;
#[cfg(test)]
#[path = "generated_tests.rs"]
mod generated_tests;
#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod pipeline_tests;
#[cfg(test)]
#[path = "reference_tests.rs"]
mod reference_tests;
#[cfg(test)]
#[path = "surface_integration_tests.rs"]
mod surface_integration_tests;
