//! Bounded GitHub issue transport for explicitly authorized feedback publication.
//!
//! Credentials remain inside this adapter. A POST is attempted once; transport or
//! protocol uncertainty is returned as ambiguous so callers reconcile its marker
//! instead of issuing another create. Production endpoints cannot be overridden.

use serde::Deserialize;

use crate::{
    domain::feedback::{FeedbackIssue, feedback_digest},
    env::FeedbackGithubToken,
    net::NetworkRuntime,
    ports::feedback::{
        FeedbackProvider, FeedbackProviderError, FeedbackProviderErrorKind, FeedbackProviderFuture,
    },
};

use super::outbound::feedback_json_client;

mod transport;

const GITHUB_API: &str = "https://api.github.com";

/// GitHub REST adapter sharing the runtime's outbound admission counters.
pub struct GithubFeedbackProvider {
    client: Result<reqwest::Client, FeedbackProviderError>,
    network: NetworkRuntime,
    token: Option<FeedbackGithubToken>,
    #[cfg(test)]
    test_endpoint: Option<String>,
}

impl GithubFeedbackProvider {
    /// Captures credential and network policy without authorizing any publication.
    pub fn new(network: NetworkRuntime, token: Option<FeedbackGithubToken>) -> Self {
        let client = feedback_json_client(&network.current().http).map_err(|_| {
            failure(
                FeedbackProviderErrorKind::Rejected,
                "feedback HTTP configuration is invalid",
            )
        });
        Self {
            client,
            network,
            token,
            #[cfg(test)]
            test_endpoint: None,
        }
    }

    async fn search_marker(
        &self,
        target: &str,
        marker: &str,
    ) -> Result<Option<FeedbackIssue>, FeedbackProviderError> {
        let nonce = marker
            .strip_prefix("<!-- relay-feedback:")
            .and_then(|text| text.strip_suffix(" -->"));
        if !nonce.is_some_and(|value| {
            matches!(value.len(), 32 | 64)
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            return Err(failure(
                FeedbackProviderErrorKind::Rejected,
                "invalid feedback correlation marker",
            ));
        }
        repository_path(target)?;
        // A random nonce avoids sending content-derived private fingerprints.
        // The durable workflow treats a missing result after an uncertain POST
        // as unresolved, so search indexing lag can never authorize a second POST.
        let query = format!(
            "repo:{target} is:issue in:body {}",
            nonce.unwrap_or_default()
        );
        let request = self
            .request(reqwest::Method::GET, "/search/issues")?
            .query(&[("q", query.as_str()), ("per_page", "2")]);
        let result: GithubSearch = self.execute(request, false).await?;
        if result.incomplete_results
            || result.total_count > 1
            || result.items.len() as u64 != result.total_count
        {
            return Err(failure(
                FeedbackProviderErrorKind::Retryable,
                "GitHub feedback search is incomplete or non-unique; publication remains blocked",
            ));
        }
        let Some(issue) = result.items.into_iter().next() else {
            return Ok(None);
        };
        if !issue
            .body
            .as_deref()
            .is_some_and(|body| body.lines().any(|line| line == marker))
        {
            return Err(failure(
                FeedbackProviderErrorKind::Retryable,
                "GitHub feedback search did not return the exact correlation marker",
            ));
        }
        issue.validated(target, None, false).map(Some)
    }
}

impl FeedbackProvider for GithubFeedbackProvider {
    fn find_marker<'a>(
        &'a self,
        target_repository: &'a str,
        marker: &'a str,
    ) -> FeedbackProviderFuture<'a, Option<FeedbackIssue>> {
        Box::pin(self.search_marker(target_repository, marker))
    }

    fn create_issue<'a>(
        &'a self,
        target_repository: &'a str,
        title: &'a str,
        body: &'a str,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue> {
        Box::pin(async move {
            if title.trim().is_empty() || title.len() > 256 || body.len() > 65_536 {
                return Err(failure(
                    FeedbackProviderErrorKind::Rejected,
                    "feedback issue payload exceeds its bounds",
                ));
            }
            let path = format!("{}/issues", repository_path(target_repository)?);
            let request = self
                .request(reqwest::Method::POST, &path)?
                .json(&serde_json::json!({"title": title, "body": body}));
            let issue: GithubIssue = self.execute(request, true).await?;
            issue.validated(target_repository, None, true)
        })
    }

    fn read_issue<'a>(
        &'a self,
        target_repository: &'a str,
        number: u64,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue> {
        Box::pin(async move {
            if number == 0 {
                return Err(failure(
                    FeedbackProviderErrorKind::Rejected,
                    "GitHub issue number must be positive",
                ));
            }
            let path = format!("{}/issues/{number}", repository_path(target_repository)?);
            let issue: GithubIssue = self
                .execute(self.request(reqwest::Method::GET, &path)?, false)
                .await?;
            issue.validated(target_repository, Some(number), false)
        })
    }
}

#[derive(Deserialize)]
struct GithubSearch {
    total_count: u64,
    incomplete_results: bool,
    items: Vec<GithubIssue>,
}

#[derive(Deserialize)]
struct GithubIssue {
    number: u64,
    html_url: String,
    state: String,
    body: Option<String>,
    pull_request: Option<serde_json::Value>,
}

impl GithubIssue {
    fn validated(
        self,
        target: &str,
        expected_number: Option<u64>,
        creating: bool,
    ) -> Result<FeedbackIssue, FeedbackProviderError> {
        let expected_url = format!("https://github.com/{target}/issues/{}", self.number);
        if self.number == 0
            || expected_number.is_some_and(|number| number != self.number)
            || self.pull_request.is_some()
            || !matches!(self.state.as_str(), "open" | "closed")
            || !self.html_url.eq_ignore_ascii_case(&expected_url)
        {
            return Err(failure(
                uncertain_kind(creating),
                "GitHub issue response does not match the authorized target",
            ));
        }
        Ok(FeedbackIssue {
            number: self.number,
            url: expected_url,
            state: self.state,
            body_digest: feedback_digest(self.body.as_deref().unwrap_or_default().as_bytes()),
        })
    }
}

fn repository_path(target: &str) -> Result<String, FeedbackProviderError> {
    let parts: Vec<_> = target.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || part.len() > 100
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(failure(
            FeedbackProviderErrorKind::Rejected,
            "GitHub target must be an owner/repository pair",
        ));
    }
    Ok(format!("/repos/{target}"))
}

fn failure(kind: FeedbackProviderErrorKind, message: &str) -> FeedbackProviderError {
    FeedbackProviderError {
        kind,
        message: message.to_owned(),
    }
}

fn uncertain_kind(creating: bool) -> FeedbackProviderErrorKind {
    if creating {
        FeedbackProviderErrorKind::Ambiguous
    } else {
        FeedbackProviderErrorKind::Retryable
    }
}

#[cfg(test)]
mod mod_tests;
#[cfg(test)]
mod test_support;
