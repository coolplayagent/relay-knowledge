pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.6-sol";
pub const DEFAULT_CODEX_REASONING_EFFORT: &str = "xhigh";

mod categories;
mod category_exclusions;
mod job_plan;
mod jobs;
mod mode;
mod model;
mod parse;
mod value_parser;

pub use categories::{CategorySet, EvaluationCategory};
pub use job_plan::JobPlan;
pub use mode::{Mode, Strategy};
pub use model::Config;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
