use super::*;

#[test]
fn score_query_decomposes_dotted_and_scoped_identifiers() {
    let dotted = ScoreQuery::new("receiver.NewFactory CreateLogs").score([
        "func NewFactory(options ...FactoryOption)",
        "receiver/receiver.go",
        "CreateLogs",
    ]);
    assert!(
        dotted >= 6.0,
        "dotted query should score receiver, NewFactory, and CreateLogs: {dotted}",
    );

    let scoped = ScoreQuery::new("rustfs_iam::error::Error as IamError")
        .score(["use rustfs_iam::error::Error as IamError;", "IamError"]);
    assert!(
        scoped >= 8.0,
        "scoped import query should score decomposed module and alias terms: {scoped}",
    );
}

#[test]
fn score_query_ignores_single_letter_path_extension_noise() {
    let query = ScoreQuery::new("linux/debugfs.h");

    assert!(query.score(["#include <linux/debugfs.h>"]) > 0.0);
    assert_eq!(query.score(["debugfs helper"]), 0.0);
    assert_eq!(query.score(["h"]), 0.0);
}

#[test]
fn score_query_preserves_score_text_semantics() {
    let query = "Cache archiveOutput";
    let fields = ["block_cache", "def archive_output_dir() -> Path:"];

    assert_eq!(
        ScoreQuery::new(query).score(fields),
        score_text(query, fields)
    );
    assert_eq!(ScoreQuery::new("   ").score(["anything"]), 0.0);
}

#[test]
fn score_query_preserves_multi_token_identifier_scores() {
    let score = ScoreQuery::new("cache output archive").score([
        "block_cache",
        "archiveOutput",
        "def archive_output_dir() -> Path:",
    ]);

    assert_eq!(score, 6.0);
}

#[test]
fn symbol_bonus_matches_scoped_query_subphrase() {
    let selector =
        crate::domain::CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
        selector,
        CodeQueryKind::Hybrid,
        10,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let bonus = symbol_query_bonus(
        &request.query,
        "Dial",
        "client.Dial",
        "func Dial(options Options) (Client, error)",
        "repo://sdk/client::Dial",
        &request,
    );

    assert!(
        bonus >= 3.0,
        "scoped query subphrase should match provider definition: {bonus}",
    );
}
