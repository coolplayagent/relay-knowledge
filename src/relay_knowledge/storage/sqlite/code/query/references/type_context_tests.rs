use super::{parameter_type_context, type_reference_usage_bonus};

#[test]
fn type_usage_prefers_direct_callable_parameters_over_nested_types() {
    let direct = type_reference_usage_bonus(
        "export function plan(instance: InstanceContext) {",
        "export function plan(instance:",
        "InstanceContext",
        None,
    )
    .expect("direct annotation should be recognized");
    let nested = type_reference_usage_bonus(
        "export function plan(input: Record<string, InstanceContext>) {",
        "export function plan(input: Record<string,",
        "InstanceContext",
        None,
    )
    .expect("nested annotation should be recognized");

    assert!(direct > nested);
}

#[test]
fn multiline_exported_parameter_context_preserves_type_affinity() {
    let context = parameter_type_context(&["export function plan("])
        .expect("open exported callable should create parameter context");
    let bonus = type_reference_usage_bonus(
        "instance: InstanceContext,",
        "instance:",
        "InstanceContext",
        Some(context),
    )
    .expect("multiline type annotation should be recognized");

    assert!(bonus > 0.0);
}
