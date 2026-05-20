use super::code_query_excerpts::{call_excerpt, reference_excerpt};

#[test]
fn call_excerpt_prefers_actual_call_line_over_type_mentions() {
    let excerpt = call_excerpt(
        Some(
            r#"
const params = {} satisfies Parameters<typeof generateObject>[0]
return yield* Effect.promise(() => generateObject(params).then((r) => r.object))
"#,
        ),
        "generate",
        "generateObject",
    );

    assert_eq!(
        excerpt,
        "generate calls generateObject: return yield* Effect.promise(() => generateObject(params).then((r) => r.object))"
    );
}

#[test]
fn call_excerpt_requires_identifier_boundaries_for_call_sites() {
    let excerpt = call_excerpt(
        Some(
            r#"
int rk_dispatch_read(
    const struct rk_driver_ops *ops)
{
    int result = ops->read(dev, buffer, length);
}
"#,
        ),
        "rk_dispatch_read",
        "read",
    );

    assert_eq!(
        excerpt,
        "rk_dispatch_read calls read: int result = ops->read(dev, buffer, length);"
    );
}

#[test]
fn reference_excerpt_includes_matching_source_line() {
    let excerpt = reference_excerpt(
        Some(
            r#"
int rk_driver_read(struct rk_device *dev)
{
    buffer[0] = (char)RK_TRACE_VALUE(dev->fd);
}
"#,
        ),
        "call",
        "RK_TRACE_VALUE",
    );

    assert_eq!(
        excerpt,
        "call reference to RK_TRACE_VALUE: buffer[0] = (char)RK_TRACE_VALUE(dev->fd);"
    );
}
