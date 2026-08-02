use super::identifier_is_plain_call;

#[test]
fn plain_call_detection_requires_real_generic_call_shape() {
    assert!(identifier_is_plain_call("(value)"));
    assert!(identifier_is_plain_call("<Payload>(value)"));
    assert!(identifier_is_plain_call("<Map<Key, Value>>(value)"));
    assert!(!identifier_is_plain_call("< computeThreshold())"));
    assert!(!identifier_is_plain_call("< bar(baz)"));
    assert!(!identifier_is_plain_call("<Payload> + value"));
}
