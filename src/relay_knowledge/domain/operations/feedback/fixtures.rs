use super::{FeedbackKind, FeedbackReport, FeedbackReproduction};

pub(super) const MARKER: &str = "<!-- relay-feedback:0123456789abcdef0123456789abcdef -->";

pub(super) fn report() -> FeedbackReport {
    FeedbackReport {
        schema_version: 1,
        kind: FeedbackKind::WorkflowFriction,
        intent: "Apply a research source batch".into(),
        expected: "One logical map publication".into(),
        actual: "Ten independent map publications".into(),
        impact: "Consumes recent history and requires a coordinating script".into(),
        observations: Vec::new(),
        evidence: Vec::new(),
        reproduction: Some(FeedbackReproduction {
            scenario: "Apply ten new source descriptions as one operation".into(),
            expected: "map version increases by one".into(),
        }),
        trace_id: None,
        request_id: None,
        diagnostics: None,
    }
}
