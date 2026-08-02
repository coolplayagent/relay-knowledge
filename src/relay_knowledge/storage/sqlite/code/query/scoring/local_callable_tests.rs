use super::*;
use crate::domain::{CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy};

#[test]
fn bonus_requires_executable_body_calls() {
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
