pub(in crate::evaluator) mod concurrency;
pub(in crate::evaluator) mod contracts;
mod finish;
mod orchestration;
pub(in crate::evaluator) mod reporting;
pub(in crate::evaluator) mod workdir;

pub use contracts::EvaluationRun;
pub use orchestration::evaluate_candidate;
