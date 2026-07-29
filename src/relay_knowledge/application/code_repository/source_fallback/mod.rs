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
mod filter_tests;
#[cfg(test)]
mod generated_tests;
#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod reference_tests;
#[cfg(test)]
mod surface_tests;
