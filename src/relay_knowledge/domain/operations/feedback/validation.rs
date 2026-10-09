//! Exact, scenario-bound comparison of independently collected regression evidence.

use super::{
    FEEDBACK_MAX_TEXT_BYTES, FeedbackError, FeedbackFix, FeedbackReport, FeedbackReproduction,
    FeedbackValidation, FeedbackValidationResult, feedback_digest, policy::bounded_text,
};

impl FeedbackReproduction {
    /// Binds the original scenario and expected output without publishing either hash.
    pub fn digest(&self) -> String {
        let mut framed = Vec::with_capacity(self.scenario.len() + self.expected.len() + 16);
        framed.extend_from_slice(&(self.scenario.len() as u64).to_be_bytes());
        framed.extend_from_slice(self.scenario.as_bytes());
        framed.extend_from_slice(&(self.expected.len() as u64).to_be_bytes());
        framed.extend_from_slice(self.expected.as_bytes());
        feedback_digest(&framed)
    }
}

/// Compares supplied evidence; the caller separately establishes runner authorization.
/// Remote issue closure and claimed pass booleans are deliberately absent.
pub fn validate_regression(
    report: &FeedbackReport,
    fix: &FeedbackFix,
    input: FeedbackValidation,
    now_ms: u64,
) -> Result<FeedbackValidationResult, FeedbackError> {
    report.validate()?;
    fix.validate()?;
    let reproduction = report.reproduction.as_ref().ok_or_else(|| {
        FeedbackError("awaiting-validation: no original scenario/criterion was declared".into())
    })?;
    if input.scenario_digest != reproduction.digest() {
        return Err(FeedbackError(
            "validation does not bind the original scenario and criterion".into(),
        ));
    }
    if input.version != fix.target_version {
        return Err(FeedbackError(
            "validation version differs from the linked fix target".into(),
        ));
    }
    for (name, value, limit) in [
        ("runner", &input.runner, 128),
        ("actual", &input.actual, FEEDBACK_MAX_TEXT_BYTES),
        ("version", &input.version, 128),
        ("environment", &input.environment, 1024),
        ("run_id", &input.run_id, 256),
    ] {
        bounded_text(name, value, limit)?;
    }
    let passed = input.actual == reproduction.expected;
    Ok(FeedbackValidationResult {
        input,
        passed,
        recorded_at_ms: now_ms,
    })
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
