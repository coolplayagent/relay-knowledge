use super::*;

#[test]
fn stop_reason_uses_default_unattended_accept_limit() {
    let config = Config::parse(vec![
        "loop".to_owned(),
        "--strategy".to_owned(),
        "unattended-layered".to_owned(),
    ])
    .expect("config should parse");
    let mut state = UnattendedState::new(100);
    state.accepted_count = UNATTENDED_ACCEPT_LIMIT;

    let reason = unattended_stop_reason(&config, &state, 120);

    assert_eq!(reason.as_deref(), Some("accepted limit reached"));
}
