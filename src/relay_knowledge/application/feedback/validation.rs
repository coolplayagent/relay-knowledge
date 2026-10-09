//! Fix associations and scenario-bound evidence supplied by an authorized runner.

use crate::domain::feedback::*;

use super::{FeedbackService, now_ms, publication::save, record_index};

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;

impl FeedbackService {
    /// Links reviewed fix metadata; linking never establishes a regression verdict.
    pub async fn link_fix(&self, id: &str, fix: FeedbackFix) -> Result<FeedbackRecord, String> {
        fix.validate().map_err(|error| error.to_string())?;
        let mut transaction = self
            .store
            .begin()
            .await
            .map_err(|error| error.to_string())?;
        let index = record_index(transaction.as_ref(), id)?;
        let mut record = transaction.snapshot().records[index].clone();
        if record.validation.fix.as_ref() == Some(&fix) {
            return Ok(record);
        }
        record.validation.fix = Some(fix);
        record.validation.state = FeedbackVerificationState::AwaitingValidation;
        save(transaction.as_mut(), index, record).await
    }

    /// Compares actual runner evidence with the original declared scenario expectation.
    /// This accepts an external authorized runner's evidence; it never executes report text.
    pub async fn validate(
        &self,
        id: &str,
        input: FeedbackValidation,
    ) -> Result<FeedbackRecord, String> {
        let mut transaction = self
            .store
            .begin()
            .await
            .map_err(|error| error.to_string())?;
        let index = record_index(transaction.as_ref(), id)?;
        let mut record = transaction.snapshot().records[index].clone();
        let fix = record
            .validation
            .fix
            .as_ref()
            .ok_or_else(|| "link a fix before recording regression evidence".to_owned())?;
        if transaction.snapshot().policy.validation_runner.as_deref() != Some(input.runner.as_str())
        {
            return Err("validation runner is not explicitly authorized; configure separate validation authority".into());
        }
        if let Some(previous) = record
            .validation
            .runs
            .iter()
            .find(|run| run.input.run_id == input.run_id)
        {
            if previous.input == input {
                return Ok(record);
            }
            return Err("validation run ID is already bound to different evidence".into());
        }
        if record.validation.runs.len() >= FEEDBACK_MAX_VALIDATION_RUNS {
            return Err(
                "validation history capacity reached; existing evidence is preserved".into(),
            );
        }
        let result = validate_regression(&record.report, fix, input, now_ms()?)
            .map_err(|error| error.to_string())?;
        record.validation.state = if result.passed {
            FeedbackVerificationState::VerifiedFixed
        } else {
            FeedbackVerificationState::Regressed
        };
        record.validation.runs.push(result);
        save(transaction.as_mut(), index, record).await
    }
}
