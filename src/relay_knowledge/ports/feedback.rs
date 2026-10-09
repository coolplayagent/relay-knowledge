//! Bounded software-feedback publication independent of provider transport.

use std::{error::Error, fmt, future::Future, pin::Pin};

use crate::domain::feedback::FeedbackIssue;

pub type FeedbackProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, FeedbackProviderError>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackProviderErrorKind {
    /// Definitive permanent rejection, with no issue created by this request.
    Rejected,
    /// Definitive pre-publication failure; a later bounded retry is safe.
    Retryable,
    /// Remote creation may have succeeded. Only reconciliation may follow.
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackProviderError {
    pub kind: FeedbackProviderErrorKind,
    pub message: String,
    /// Provider's earliest safe retry time in Unix milliseconds; never sleeps in transport.
    pub retry_not_before_ms: Option<u64>,
}

impl fmt::Display for FeedbackProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for FeedbackProviderError {}

/// Every target is validated policy data, never extracted from report text.
pub trait FeedbackProvider: Send + Sync {
    fn find_marker<'a>(
        &'a self,
        target_repository: &'a str,
        marker: &'a str,
    ) -> FeedbackProviderFuture<'a, Option<FeedbackIssue>>;

    fn create_issue<'a>(
        &'a self,
        target_repository: &'a str,
        title: &'a str,
        body: &'a str,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue>;

    fn read_issue<'a>(
        &'a self,
        target_repository: &'a str,
        number: u64,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue>;
}
