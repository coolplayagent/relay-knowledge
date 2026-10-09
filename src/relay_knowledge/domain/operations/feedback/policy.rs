//! Strict request bounds and explicit publication authorization.

use super::{
    FEEDBACK_MAX_EVIDENCE_COUNT, FEEDBACK_MAX_REPORT_BYTES, FEEDBACK_MAX_TEXT_BYTES,
    FEEDBACK_SCHEMA_VERSION, FeedbackError, FeedbackFix, FeedbackMode, FeedbackPolicy,
    FeedbackReport, feedback_digest,
};

impl FeedbackPolicy {
    pub fn validate(&self) -> Result<(), FeedbackError> {
        schema_version(self.schema_version)?;
        if let Some(runner) = &self.validation_runner {
            bounded_text("validation_runner", runner, 128)?;
        }
        if self.daily_quota == 0 || self.daily_quota > 100 {
            return Err(FeedbackError("daily_quota must be within 1..=100".into()));
        }
        if self.allowed_kinds.len() > 6 {
            return Err(FeedbackError("allowed_kinds exceeds six kinds".into()));
        }
        for (index, kind) in self.allowed_kinds.iter().enumerate() {
            if self.allowed_kinds[..index].contains(kind) {
                return Err(FeedbackError("allowed_kinds contains duplicates".into()));
            }
        }
        if let Some(repository) = &self.target_repository {
            let Some((owner, name)) = repository.split_once('/') else {
                return Err(FeedbackError(
                    "target_repository must be owner/repository".into(),
                ));
            };
            if !repository_component(owner) || !repository_component(name) {
                return Err(FeedbackError(
                    "target_repository has an invalid component".into(),
                ));
            }
        }
        if self.mode == FeedbackMode::AutoSubmit
            && (self.target_repository.is_none() || self.allowed_kinds.is_empty())
        {
            return Err(FeedbackError(
                "auto-submit requires a target repository and allowed kinds".into(),
            ));
        }
        Ok(())
    }
}

fn repository_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

impl FeedbackReport {
    pub fn validate(&self) -> Result<(), FeedbackError> {
        schema_version(self.schema_version)?;
        for (name, text) in [
            ("intent", &self.intent),
            ("expected", &self.expected),
            ("actual", &self.actual),
            ("impact", &self.impact),
        ] {
            bounded_text(name, text, FEEDBACK_MAX_TEXT_BYTES)?;
        }
        if self.observations.len() > FEEDBACK_MAX_EVIDENCE_COUNT
            || self.evidence.len() > FEEDBACK_MAX_EVIDENCE_COUNT
        {
            return Err(FeedbackError(
                "at most sixteen observations/evidence entries are allowed".into(),
            ));
        }
        for observation in &self.observations {
            bounded_text("observation", &observation.text, FEEDBACK_MAX_TEXT_BYTES)?;
        }
        for evidence in &self.evidence {
            bounded_text("evidence label", &evidence.label, 256)?;
            bounded_text("evidence content", &evidence.content, 16_384)?;
        }
        for (name, value) in [
            ("trace_id", &self.trace_id),
            ("request_id", &self.request_id),
        ] {
            if let Some(value) = value {
                bounded_text(name, value, 256)?;
            }
        }
        if let Some(reproduction) = &self.reproduction {
            bounded_text(
                "reproduction scenario",
                &reproduction.scenario,
                FEEDBACK_MAX_TEXT_BYTES,
            )?;
            bounded_text(
                "reproduction expected",
                &reproduction.expected,
                FEEDBACK_MAX_TEXT_BYTES,
            )?;
        }
        let bytes = serde_json::to_vec(self).map_err(|error| FeedbackError(error.to_string()))?;
        if bytes.len() > FEEDBACK_MAX_REPORT_BYTES {
            return Err(FeedbackError("feedback report exceeds 65536 bytes".into()));
        }
        Ok(())
    }

    /// Deduplicates the original scenario/version, excluding per-occurrence trace data.
    pub fn fingerprint(&self, cli_version: &str) -> Result<String, FeedbackError> {
        self.validate()?;
        let canonical = serde_json::to_vec(&(
            self.schema_version,
            self.kind,
            &self.intent,
            &self.expected,
            &self.actual,
            &self.impact,
            &self.observations,
            &self.reproduction,
            cli_version,
        ))
        .map_err(|error| FeedbackError(error.to_string()))?;
        Ok(feedback_digest(&canonical))
    }
}

impl FeedbackFix {
    pub fn validate(&self) -> Result<(), FeedbackError> {
        bounded_text("fix reference", &self.reference, 1024)?;
        bounded_text("target version", &self.target_version, 128)
    }
}

pub(super) fn bounded_text(name: &str, value: &str, limit: usize) -> Result<(), FeedbackError> {
    if value.trim().is_empty() || value.len() > limit || value.contains('\0') {
        return Err(FeedbackError(format!(
            "{name} must contain 1..={limit} UTF-8 bytes without NUL"
        )));
    }
    Ok(())
}

fn schema_version(version: u32) -> Result<(), FeedbackError> {
    if version != FEEDBACK_SCHEMA_VERSION {
        return Err(FeedbackError(
            "unsupported feedback schema_version; expected 1".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
