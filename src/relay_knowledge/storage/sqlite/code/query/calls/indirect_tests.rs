use super::indirect_call_binding_fields;

#[test]
fn indirect_binding_fields_accept_member_assignments() {
    let fields = indirect_call_binding_fields(
        "static const struct ops table = {\n    .read = rk_driver_read,\n};",
        "rk_driver_read",
    );

    assert_eq!(fields, vec!["read".to_owned()]);
}

#[test]
fn indirect_binding_fields_ignore_function_call_wrappers() {
    let fields = indirect_call_binding_fields(
        "return yield* Effect.promise(() => generateObject(params).then((r) => r.object))",
        "generateObject",
    );

    assert!(fields.is_empty(), "{fields:?}");
}
