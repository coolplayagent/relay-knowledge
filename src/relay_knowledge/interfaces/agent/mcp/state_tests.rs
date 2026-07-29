use axum::http::HeaderMap;

use super::*;

#[test]
fn session_ids_are_unpredictable_header_safe_values() {
    let sessions = SessionRegistry::default();
    let first = create_session(&sessions);
    let second = create_session(&sessions);

    assert_ne!(first, second);
    assert!(is_session_id(&first));
    assert!(is_session_id(&second));
}

#[test]
fn issued_sessions_resolve_stable_namespaces_and_track_initialization() {
    let sessions = SessionRegistry::default();
    let session_id = create_session(&sessions);
    let headers = session_headers(&session_id);

    let before_initialized = sessions
        .require_session(&headers)
        .expect("issued session should resolve");
    assert_eq!(
        before_initialized.namespace(),
        format!("session:{session_id}")
    );
    assert!(!before_initialized.initialized);

    sessions
        .mark_initialized(before_initialized.session_id())
        .expect("session should initialize");
    let after_initialized = sessions
        .require_session(&headers)
        .expect("initialized session should resolve");
    assert!(after_initialized.initialized);
}

#[test]
fn missing_or_unknown_session_headers_are_rejected() {
    let sessions = SessionRegistry::default();
    let unknown_headers = session_headers("rk-unissued");

    assert_eq!(
        sessions
            .require_session(&HeaderMap::new())
            .expect_err("missing header should fail"),
        SessionLookupError::Missing
    );
    assert_eq!(
        sessions
            .require_session(&unknown_headers)
            .expect_err("unknown header should fail"),
        SessionLookupError::Unknown
    );
}

#[test]
fn session_eviction_preserves_recently_used_sessions() {
    let sessions = SessionRegistry::default();
    let active = create_session(&sessions);
    let stale = create_session(&sessions);
    for _ in 0..(MAX_TRACKED_SESSIONS - 2) {
        create_session(&sessions);
    }
    sessions
        .require_session(&session_headers(&active))
        .expect("active session should touch recency");

    let newest = create_session(&sessions);

    assert!(sessions.contains_session(&active));
    assert!(sessions.contains_session(&newest));
    assert!(!sessions.contains_session(&stale));
    assert_eq!(sessions.tracked_len(), MAX_TRACKED_SESSIONS);
}

#[test]
fn session_usage_history_is_bounded_for_stable_sessions() {
    let sessions = SessionRegistry::default();
    let session_id = create_session(&sessions);
    let headers = session_headers(&session_id);

    for _ in 0..(MAX_TRACKED_SESSIONS * 3) {
        sessions
            .require_session(&headers)
            .expect("stable session should resolve");
    }

    assert!(sessions.usage_history_len() <= MAX_TRACKED_SESSIONS * 2);
}

#[test]
fn cancellation_requests_are_idempotent_while_active() {
    let registry = CancellationRegistry::default();
    let (_receiver, _registration) = registry.register("string:call".to_owned());

    assert!(registry.cancel("string:call"));
    assert!(!registry.cancel("string:call"));
}

fn create_session(sessions: &SessionRegistry) -> String {
    sessions
        .create_session()
        .expect("OS entropy should create session")
}

fn session_headers(session_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(MCP_SESSION_ID_HEADER, session_id.parse().unwrap());
    headers
}

fn is_session_id(value: &str) -> bool {
    value.len() == 67
        && value.starts_with("rk-")
        && value[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
