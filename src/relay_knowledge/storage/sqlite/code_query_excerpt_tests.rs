use super::*;

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
