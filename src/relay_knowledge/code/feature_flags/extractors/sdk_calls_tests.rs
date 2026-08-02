use std::collections::BTreeMap;

use super::{
    sdk_continued_flag_key, sdk_flag_keys_for_line, sdk_next_pending_argument_index,
    sdk_pending_argument_index,
};

#[test]
fn sdk_receiver_tracking_requires_a_provider_assignment() {
    let mut receivers = BTreeMap::new();

    assert!(
        sdk_flag_keys_for_line("let client = OpenFeature.getClient();", &mut receivers, 0,)
            .is_empty()
    );
    assert_eq!(
        sdk_flag_keys_for_line(
            r#"client.getBooleanValue("checkout.enabled", false);"#,
            &mut receivers,
            0,
        ),
        ["checkout.enabled"]
    );
}

#[test]
fn sdk_reassignment_removes_a_tracked_receiver() {
    let mut receivers = BTreeMap::new();
    sdk_flag_keys_for_line("let client = OpenFeature.getClient();", &mut receivers, 0);

    sdk_flag_keys_for_line("client = unrelated_factory();", &mut receivers, 0);

    assert!(
        sdk_flag_keys_for_line(
            r#"client.getBooleanValue("ignored", false);"#,
            &mut receivers,
            0,
        )
        .is_empty()
    );
}

#[test]
fn multiline_sdk_arguments_keep_the_target_position() {
    let receivers = BTreeMap::from([("client".to_owned(), 0)]);

    assert_eq!(
        sdk_pending_argument_index("client.isEnabled(", &receivers),
        Some(0)
    );
    assert_eq!(
        sdk_next_pending_argument_index("context,", 0),
        None,
        "a completed non-target argument cannot become the flag key"
    );
    assert_eq!(
        sdk_continued_flag_key(r#""checkout.enabled", false)"#, 0),
        Some("checkout.enabled".to_owned())
    );
}
