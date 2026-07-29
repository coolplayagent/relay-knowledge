use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn caller_identity_fast_path_requires_bounded_exact_target_hits() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let callers_request = CodeRetrievalRequest::new(
        "TargetThing",
        selector.clone(),
        CodeQueryKind::Callers,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let callees_request = CodeRetrievalRequest::new(
        "TargetThing",
        selector,
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let callers_identity =
        call_identity_query(&callers_request).expect("callers identity should parse");
    let callees_identity =
        call_identity_query(&callees_request).expect("callees identity should parse");

    assert!(call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        3,
        false
    ));
    assert!(!call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        11,
        false
    ));
    assert!(!call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        3,
        true
    ));
    assert!(call_identity_hits_can_answer_without_fts(
        &callees_request,
        &callees_identity,
        3,
        false
    ));
    let broad_identity = call_identity_query(
        &CodeRetrievalRequest::new(
            "Table",
            CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
                .expect("selector should validate"),
            CodeQueryKind::Callees,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate"),
    )
    .expect("identity query should parse");
    assert!(!call_identity_hits_can_answer_without_fts(
        &callees_request,
        &broad_identity,
        1,
        false
    ));

    let narrowed_selector = CodeRepositorySelector::new(
        "repo",
        "commit",
        vec!["src/table.cc".to_owned()],
        vec!["cpp".to_owned()],
    )
    .expect("selector should validate");
    let narrowed_request = CodeRetrievalRequest::new(
        "Run",
        narrowed_selector,
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let narrowed_identity =
        call_identity_query(&narrowed_request).expect("identity query should parse");

    assert!(call_identity_hits_can_answer_without_fts(
        &narrowed_request,
        &narrowed_identity,
        2,
        false
    ));
}

#[test]
fn call_display_name_includes_nested_owner_context() {
    assert_eq!(
        call_display_name(
            Some("connection"),
            Some("repo://repo/frontend::core::stream::attachRunStream.connection"),
        )
        .as_deref(),
        Some("attachRunStream.connection")
    );
    assert_eq!(
        call_display_name(
            Some("dispatch"),
            Some("repo://repo/src::main::ServiceFactory.dispatch"),
        )
        .as_deref(),
        Some("dispatch")
    );
    assert_eq!(
        call_display_name(
            None,
            Some("repo://repo/src::main::ServiceFactory.exactOwner"),
        )
        .as_deref(),
        Some("exactOwner")
    );
    assert_eq!(
        call_display_name(
            Some("endStream"),
            Some("repo://repo/frontend::stream.endStream")
        )
        .as_deref(),
        Some("endStream")
    );
    assert_eq!(
        inferred_caller_name_from_excerpt(Some(
            "Status Table::InternalGet(const ReadOptions& options) {\nreturn Status::OK();\n}"
        ))
        .as_deref(),
        Some("InternalGet")
    );
}

#[test]
fn local_callable_bonus_requires_executable_body_calls() {
    let request = CodeRetrievalRequest::new(
        "RuntimeService.Dispatch",
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let identity_lambda = "public void Dispatch(BufferPoolSink sink, int size) {\n\
        var buffer = sink.RentBuffer(size);\n\
        Func<byte[], byte[]> returnBuffer = rented => rented;\n\
        sink.Write(returnBuffer(buffer));\n\
    }";
    let executable_lambda = "int RunPipeline(Cache<std::string>& cache) {\n\
        auto append_event = [&cache](const PipelineEvent& event) {\n\
          cache.Insert(event.key);\n\
          return event.size;\n\
        };\n\
        return append_event(event);\n\
    }";

    assert_eq!(
        local_callable_declaration_bonus(8.0, Some(identity_lambda), "returnBuffer", &request),
        0.0
    );
    assert_eq!(
        local_callable_declaration_bonus(8.0, Some(executable_lambda), "append_event", &request),
        LOCAL_CALLABLE_DECLARATION_BONUS
    );
}
