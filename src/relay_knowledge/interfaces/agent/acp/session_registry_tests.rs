use crate::api::AgentProtocolKind;

use super::*;

#[test]
fn session_records_normalize_client_identity() {
    let registry = AcpSessionRegistry::default();
    registry.insert_session(
        "session-1".to_owned(),
        AcpSessionRecord::new(
            Some("  editor  ".to_owned()),
            Some("   ".to_owned()),
            Some(" actor-1 ".to_owned()),
        ),
    );

    let record = registry.session("session-1").expect("session should exist");
    let identity = record.identity("session-1", Some("request-1".to_owned()));
    assert_eq!(identity.protocol, AgentProtocolKind::Acp);
    assert_eq!(identity.client_name.as_deref(), Some("editor"));
    assert_eq!(identity.client_version, None);
    assert_eq!(identity.actor_id.as_deref(), Some("actor-1"));
    assert_eq!(identity.session_id.as_deref(), Some("session-1"));
    assert_eq!(identity.tool_call_id.as_deref(), Some("request-1"));
}

#[test]
fn active_request_drop_removes_cancellation_registration() {
    let registry = AcpSessionRegistry::default();
    let (_receiver, registration) = registry.register_request("session-1", "request-1".to_owned());

    drop(registration);

    assert!(!registry.cancel_request("session-1", "request-1"));
}

#[test]
fn cancellation_notifies_active_request_until_release() {
    let registry = AcpSessionRegistry::default();
    let (receiver, registration) = registry.register_request("session-1", "request-1".to_owned());

    assert!(registry.cancel_request("session-1", "request-1"));
    assert!(*receiver.borrow());
    registration.release();
    assert!(!registry.cancel_request("session-1", "request-1"));
}
