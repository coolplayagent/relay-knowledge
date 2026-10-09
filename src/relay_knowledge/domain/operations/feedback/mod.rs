//! Software experience feedback contracts, separate from accepted graph facts.

mod evidence;
mod policy;
mod types;
mod validation;

pub use evidence::{feedback_digest, prepare_payload};
pub use types::*;
pub use validation::validate_regression;

#[cfg(test)]
#[path = "fixtures.rs"]
mod fixtures;
