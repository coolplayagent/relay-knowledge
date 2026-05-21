use super::code_query_excerpts::{call_excerpt, callee_excerpt, reference_excerpt};

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
fn call_excerpt_prefers_local_callable_declaration_over_later_invocation() {
    let excerpt = call_excerpt(
        Some(
            r#"
auto append_event = [&cache, &pipeline](const PipelineEvent& event) {
    cache.Insert(event.key);
    return pipeline(event);
};
total += append_event(event);
"#,
        ),
        "RunPipeline",
        "append_event",
    );

    assert_eq!(
        excerpt,
        "RunPipeline calls append_event: auto append_event = [&cache, &pipeline](const PipelineEvent& event) {"
    );
}

#[test]
fn callee_excerpt_includes_inline_callable_body_context() {
    let excerpt = callee_excerpt(
        Some(
            r#"
auto append_event = [&cache, &pipeline](const PipelineEvent& event) {
    cache.Insert(event.key);
    return pipeline(event);
};
total += append_event(event);
"#,
        ),
        "RunPipeline",
        "append_event",
    );

    assert_eq!(
        excerpt,
        "RunPipeline calls append_event: auto append_event = [&cache, &pipeline](const PipelineEvent& event) { cache.Insert(event.key); return pipeline(event); };"
    );
    assert!(!excerpt.contains("total += append_event"));
}

#[test]
fn callee_excerpt_includes_bounded_execution_context_after_call_site() {
    let excerpt = callee_excerpt(
        Some(
            r#"
int rk_dispatch_read(struct rk_driver_ops *ops) {
    if (!rk_validate_device(dev)) {
        return -EINVAL;
    }
    if (ops->open(dev) < 0) {
        return -EIO;
    }
    if (rk_lock_device(dev) < 0) {
        return -EBUSY;
    }
    int result = ops->read(dev, buffer, length);
    rk_unlock_device(dev);
    return result;
}
"#,
        ),
        "rk_dispatch_read",
        "rk_validate_device",
    );

    assert!(excerpt.contains("rk_validate_device(dev)"));
    assert!(excerpt.contains("ops->open(dev)"));
    assert!(excerpt.contains("ops->read(dev, buffer, length)"));
    assert!(!excerpt.contains("rk_unlock_device(dev)"));
    assert!(!excerpt.contains("return result"));
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
