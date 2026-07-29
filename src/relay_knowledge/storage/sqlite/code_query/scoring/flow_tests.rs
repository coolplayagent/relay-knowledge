use super::super::code_query_caller_context_scoring::caller_context_density_bonus;
use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn caller_context_bonus_prefers_target_named_surfaces() {
    let request = request("redactUrl", CodeQueryKind::Callers);

    let redactor = caller_context_density_bonus(
        4.0,
        "redactUrl",
        Some("url"),
        "redactUrl",
        "packages/http-recorder/src/redactor.ts",
        Some("request: (snapshot) => ({ url: redactUrl(snapshot.url) })"),
        &request,
    );
    let executor = caller_context_density_bonus(
        4.0,
        "redactUrl",
        Some("requestDetails"),
        "redactUrl",
        "packages/llm/src/route/executor.ts",
        Some("url: redactUrl(request.url),"),
        &request,
    );

    assert!(redactor > executor);
}

#[test]
fn execution_flow_bonus_requires_flow_intent_and_content_coverage() {
    let hybrid = request(
        "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
        CodeQueryKind::Hybrid,
    );
    let focused = execution_flow_chunk_bonus(
        8.0,
        &hybrid.query,
        "OpenAI Chat protocol route uses SSE transport events.\nconst step = () => ToolStream.empty()\nLifecycle.finish(lifecycle, events)",
        "src/openai-chat.ts",
        &hybrid,
    );
    let narrow = execution_flow_chunk_bonus(
        8.0,
        &hybrid.query,
        "const lowerToolCall = () => ({ type: \"function\" })",
        "src/openai-chat.ts",
        &hybrid,
    );
    let symbol = request("OpenAI Chat", CodeQueryKind::Definition);

    assert!(focused > narrow);
    assert_eq!(
        execution_flow_chunk_bonus(
            8.0,
            &symbol.query,
            "OpenAI Chat protocol route",
            "src/openai-chat.ts",
            &symbol
        ),
        0.0
    );
}

#[test]
fn execution_flow_bonus_prefers_tool_lifecycle_finalization_steps() {
    let hybrid = request(
        "OpenAI Chat protocol sse tool call delta lifecycle finish events",
        CodeQueryKind::Hybrid,
    );
    let finalization = execution_flow_chunk_bonus(
        8.0,
        &hybrid.query,
        "const finished = finishReason !== undefined\n  ? yield* ToolStream.finishAll(ADAPTER, tools)\n  : undefined\nreturn [{ toolCallEvents: finished?.events, lifecycle }, events]",
        "packages/llm/src/protocols/openai-chat.ts",
        &hybrid,
    );
    let wrapper = execution_flow_chunk_bonus(
        8.0,
        &hybrid.query,
        "export const protocol = Protocol.make({ stream: { initial: () => ({ tools: ToolStream.empty(), lifecycle: Lifecycle.initial() }) } })",
        "packages/llm/src/protocols/openai-chat.ts",
        &hybrid,
    );

    assert!(
        finalization > wrapper,
        "finalization={finalization} wrapper={wrapper}"
    );

    let finalize = request(
        "OpenAI Chat protocol sse tool call delta lifecycle finalized events",
        CodeQueryKind::Hybrid,
    );
    assert!(
        execution_flow_chunk_bonus(
            8.0,
            &finalize.query,
            "yield* Lifecycle.finalize(lifecycle, events)",
            "packages/llm/src/protocols/openai-chat.ts",
            &finalize,
        ) > 0.0
    );
}

#[test]
fn execution_flow_bonus_ignores_tests_without_test_intent() {
    let hybrid = request(
        "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
        CodeQueryKind::Hybrid,
    );

    assert_eq!(
        execution_flow_chunk_bonus(
            8.0,
            &hybrid.query,
            "OpenAI Chat protocol route uses SSE transport events.\nLifecycle.finish(lifecycle, events)",
            "packages/llm/test/openai-chat.test.ts",
            &hybrid,
        ),
        0.0
    );
}

#[test]
fn inline_construct_bonus_prefers_callback_shapes() {
    let hybrid = request(
        "Project db helper Database.use inline callback Effect.sync",
        CodeQueryKind::Hybrid,
    );
    let inline = inline_construct_chunk_bonus(
        8.0,
        &hybrid.query,
        "const db = <T>(fn: Fn<T>) => Effect.sync(() => Database.use(fn))",
        "packages/opencode/src/project/project.ts",
        &hybrid,
    );
    let named = inline_construct_chunk_bonus(
        8.0,
        &hybrid.query,
        "function db(fn) { return Effect.sync(Database.use(fn)) }",
        "packages/opencode/src/project/project.ts",
        &hybrid,
    );

    assert!(inline > named, "inline={inline} named={named}");
    assert_eq!(named, 0.0);
}

#[test]
fn inline_construct_bonus_detects_cross_language_forms() {
    for (query, content) in [
        (
            "ReflectionUtils USER_DECLARED_METHODS MethodFilter lambda isBridge isSynthetic",
            "MethodFilter USER_DECLARED_METHODS = method -> !method.isBridge() && !method.isSynthetic()",
        ),
        (
            "ResourceEventHandlerFuncs AddFunc inline addNode UpdateFunc",
            "ResourceEventHandlerFuncs{AddFunc: func(obj interface{}) { ttlc.addNode(obj) }}",
        ),
        (
            "keystone_roles roles.iter any admin reseller_admin closure",
            "keystone_roles.iter().any(|role| role == \"admin\" || role == \"reseller_admin\")",
        ),
        (
            "_handle_connection nested async recv_json send_event websocket",
            "async def recv_json():\n    await websocket.recv()\nasync def send_event(): pass",
        ),
        (
            "round_hint_to_min static inline do_mmap mmap_min_addr",
            "static inline unsigned long round_hint_to_min(unsigned long addr) { return mmap_min_addr; }",
        ),
        (
            "setupEventBindings promptInput addEventListener input arrow callback handlePromptComposerInput",
            "promptInput.addEventListener(\"input\", () => handlePromptComposerInput())",
        ),
    ] {
        let request = request(query, CodeQueryKind::Hybrid);
        let bonus = inline_construct_chunk_bonus(
            8.0,
            &request.query,
            content,
            "src/inline-source.rs",
            &request,
        );
        assert!(bonus > 0.0, "missing inline bonus for {content}");
    }
}

#[test]
fn inline_construct_bonus_ignores_tests_without_test_intent() {
    let hybrid = request(
        "Project db helper Database.use inline callback Effect.sync",
        CodeQueryKind::Hybrid,
    );

    assert_eq!(
        inline_construct_chunk_bonus(
            8.0,
            &hybrid.query,
            "const db = <T>(fn: Fn<T>) => Effect.sync(() => Database.use(fn))",
            "packages/opencode/src/project/project.test.ts",
            &hybrid,
        ),
        0.0
    );
}

#[test]
fn inline_construct_bonus_allows_benchmark_implementation_sources() {
    let hybrid = request(
        "ZstdCompress lambda port::Zstd_Compress FLAGS_zstd_compression_level",
        CodeQueryKind::Hybrid,
    );
    let bonus = inline_construct_chunk_bonus(
        8.0,
        &hybrid.query,
        "auto ZstdCompress = [](const char* input, size_t length) { return port::Zstd_Compress(input, length, FLAGS_zstd_compression_level); };",
        "benchmarks/db_bench.cc",
        &hybrid,
    );

    assert!(bonus > 0.0, "benchmark lambda should be eligible");
}

#[test]
fn compact_high_coverage_bonus_prefers_concise_usage_chunks() {
    let hybrid = request(
        "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
        CodeQueryKind::Hybrid,
    );
    let compact = compact_high_coverage_chunk_bonus(
        8.0,
        &hybrid.query,
        "func main() {\n\
            c, err := client.Dial(envconfig.MustLoadDefaultClientOptions())\n\
            if err != nil { panic(err) }\n\
            w := worker.New(c, \"hello-world\", worker.Options{})\n\
            w.RegisterWorkflow(helloworld.Workflow)\n\
            err = w.Run(worker.InterruptCh())\n\
            if err != nil { panic(err) }\n\
            }",
        "helloworld/worker/main.go",
        &hybrid,
    );
    let long = compact_high_coverage_chunk_bonus(
        8.0,
        &hybrid.query,
        &(0..25)
            .map(|_| "client.Dial envconfig MustLoadDefaultClientOptions workflow client")
            .collect::<Vec<_>>()
            .join("\n"),
        "starter/main.go",
        &hybrid,
    );
    let definition = request("client.Dial workflow client", CodeQueryKind::Definition);

    assert!(
        compact > 0.0,
        "compact chunk should receive a bounded bonus"
    );
    assert_eq!(long, 0.0);
    assert_eq!(
        compact_high_coverage_chunk_bonus(
            8.0,
            &definition.query,
            "client.Dial workflow client",
            "src/client.go",
            &definition,
        ),
        0.0
    );
}

#[test]
fn compact_api_sequence_bonus_prefers_complete_short_lifecycle_flows() {
    let hybrid = request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Hybrid,
    );
    let complete = compact_api_sequence_chunk_bonus(
        8.0,
        &hybrid.query,
        "func main() {\n\
            w := worker.New(c, \"hello-world\", worker.Options{})\n\
            w.RegisterWorkflow(helloworld.Workflow)\n\
            w.RegisterActivity(helloworld.Activity)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
        "helloworld/worker/main.go",
        &hybrid,
    );
    let partial = compact_api_sequence_chunk_bonus(
        8.0,
        &hybrid.query,
        "func main() {\n\
            w := worker.New(c, caller.TaskQueue, worker.Options{})\n\
            w.RegisterWorkflow(caller.EchoCallerWorkflow)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
        "nexus/caller/worker/main.go",
        &hybrid,
    );
    let verbose = compact_api_sequence_chunk_bonus(
        8.0,
        &hybrid.query,
        &(0..24)
            .map(|_| "w.RegisterWorkflow(flow.Workflow); w.RegisterActivity(flow.Activity)")
            .collect::<Vec<_>>()
            .join("\n"),
        "worker-specific-task-queues/worker/main.go",
        &hybrid,
    );

    assert!(complete > partial, "complete={complete} partial={partial}");
    assert!(
        partial > 0.0,
        "partial compact API sequence should still score"
    );
    assert_eq!(verbose, 0.0);
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}
