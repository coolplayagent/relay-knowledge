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
