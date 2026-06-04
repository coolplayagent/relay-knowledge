use super::*;

#[test]
fn caller_result_assignment_bonus_requires_assignment_shape_and_query_intent() {
    let callers = request("createPool", CodeQueryKind::Callers);
    let callees = request("createPool", CodeQueryKind::Callees);
    let production_intent = CallSiteQueryIntent {
        test_or_benchmark: false,
        example_or_sample: false,
    };
    let example_intent = CallSiteQueryIntent {
        test_or_benchmark: false,
        example_or_sample: true,
    };

    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "src/runtime/cache_config.ts",
            "createPool",
            Some("settings.pool = createPool(options)"),
            "createPool",
            &callers,
            production_intent,
        ),
        1.4
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "db/db_impl.cc",
            "NewLRUCache",
            Some("result.block_cache = NewLRUCache(8 << 20);"),
            "NewLRUCache",
            &callers,
            production_intent,
        ),
        1.4
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "src/runtime/client.go",
            "Dial",
            Some("client, err := Dial(options)"),
            "Dial",
            &callers,
            production_intent,
        ),
        1.15
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "src/runtime/cache_config.ts",
            "createPool",
            Some("if (current == createPool(options)) return"),
            "createPool",
            &callers,
            production_intent,
        ),
        0.0
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "src/runtime/cache_config.ts",
            "createPool",
            Some("settings.pool = recreatePool(options)"),
            "createPool",
            &callers,
            production_intent,
        ),
        0.0
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "src/runtime/cache_config.ts",
            "createPool",
            Some("settings.factory = createPool(options)"),
            "createPool",
            &callers,
            production_intent,
        ),
        1.15
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "examples/cache_demo.ts",
            "createPool",
            Some("settings.pool = createPool(options)"),
            "createPool",
            &callers,
            production_intent,
        ),
        0.0
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "examples/cache_demo.ts",
            "createPool example",
            Some("settings.pool = createPool(options)"),
            "createPool",
            &callers,
            example_intent,
        ),
        1.4
    );
    assert_eq!(
        caller_result_assignment_bonus(
            4.0,
            "src/runtime/cache_config.ts",
            "createPool",
            Some("settings.pool = createPool(options)"),
            "createPool",
            &callees,
            production_intent,
        ),
        0.0
    );
}

#[test]
fn callee_member_context_bonus_requires_member_call_shape() {
    let callees = request("OwnerTarget", CodeQueryKind::Callees);
    let callers = request("OwnerTarget", CodeQueryKind::Callers);
    let exact_caller = request("streamWith", CodeQueryKind::Callees);

    assert_eq!(
        callee_member_context_bonus(
            4.0,
            Some("return ToolRuntime.stream({ input })"),
            "stream",
            &callees,
        ),
        0.45
    );
    assert_eq!(
        callee_member_context_bonus(
            4.0,
            Some("return ToolRuntime::stream(input)"),
            "stream",
            &callees,
        ),
        0.45
    );
    assert_eq!(
        callee_member_context_bonus(4.0, Some("stream: streamRequest"), "stream", &callees,),
        0.0
    );
    assert_eq!(
        callee_member_context_bonus(4.0, Some("stream(input)"), "stream", &callees),
        0.0
    );
    assert_eq!(
        callee_member_context_bonus(
            4.0,
            Some("return ToolRuntime.stream({ input })"),
            "stream",
            &callers,
        ),
        0.0
    );
    assert_eq!(
        exact_caller_named_receiver_member_call_bonus(
            4.0,
            "streamWith",
            Some("streamWith"),
            Some("return ToolRuntime.stream({ input })"),
            "stream",
            &exact_caller,
        ),
        5.0
    );
    assert_eq!(
        exact_caller_named_receiver_member_call_bonus(
            4.0,
            "ServiceFactory.dispatch",
            Some("dispatch"),
            Some("return create().handle(value)"),
            "handle",
            &request("ServiceFactory.dispatch", CodeQueryKind::Callees),
        ),
        0.0
    );
    assert_eq!(
        exact_caller_named_receiver_member_call_bonus(
            4.0,
            "rk_dispatch_read",
            Some("rk_dispatch_read"),
            Some("if (ops->open(dev) < 0) return -1;"),
            "open",
            &request("rk_dispatch_read", CodeQueryKind::Callees),
        ),
        0.0
    );
}
