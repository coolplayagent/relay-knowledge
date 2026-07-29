use std::{collections::BTreeSet, path::PathBuf};

pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.5";
pub const DEFAULT_CODEX_REASONING_EFFORT: &str = "xhigh";

include!("mode.rs");
include!("jobs.rs");
include!("categories.rs");
include!("model.rs");
include!("parse.rs");
include!("category_exclusions.rs");
include!("job_plan.rs");
include!("value_parser.rs");

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
