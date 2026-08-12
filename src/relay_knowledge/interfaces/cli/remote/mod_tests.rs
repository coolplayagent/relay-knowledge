//! Direct contracts for the remote CLI transport owner.

use super::*;

#[test]
fn status_error_maps_http_429_to_qos_rejected() {
    let error = status_error(
        StatusCode::TOO_MANY_REQUESTS,
        std::borrow::Cow::Borrowed("request budget exhausted"),
    );

    assert_eq!(error.error_kind, ErrorKind::QosRejected);
    assert!(error.message.contains("request budget exhausted"));
}

#[test]
fn repository_update_is_remote_and_never_falls_back_to_local_state() {
    let action = CliAction::Repo(RepoCommand::Update {
        alias: "core".to_owned(),
        base_ref: None,
        head_ref: None,
    });

    assert!(supports(&action));
    assert!(blocks_local_fallback(&action));
}
