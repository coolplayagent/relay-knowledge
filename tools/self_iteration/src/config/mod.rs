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
mod tests {
    use super::*;

    include!("parse_tests.rs");
    include!("category_tests.rs");
    include!("documentation_tests.rs");
    include!("unattended_tests.rs");
    include!("job_plan_tests.rs");
    include!("test_support.rs");
}
