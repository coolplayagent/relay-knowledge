use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn unique_api_sequence_bonus_prefers_complete_non_repeated_flows() {
    let request = request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Hybrid,
    );
    let compact = compact_unique_api_sequence_chunk_bonus(
        8.0,
        &request.query,
        "func main() {\n\
            w := worker.New(c, \"hello-world\", worker.Options{})\n\
            w.RegisterWorkflow(helloworld.Workflow)\n\
            w.RegisterActivity(helloworld.Activity)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
        "helloworld/worker/main.go",
        &request,
    );
    let repeated = compact_unique_api_sequence_chunk_bonus(
        8.0,
        &request.query,
        "func main() {\n\
            w := worker.New(c, queue.Name, worker.Options{})\n\
            w.RegisterWorkflow(flow.Workflow)\n\
            w.RegisterActivity(flow.FirstActivity)\n\
            w.RegisterActivity(flow.SecondActivity)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
        "workflow/worker/main.go",
        &request,
    );
    let partial = compact_unique_api_sequence_chunk_bonus(
        8.0,
        &request.query,
        "func main() {\n\
            w := worker.New(c, queue.Name, worker.Options{})\n\
            w.RegisterWorkflow(flow.Workflow)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
        "workflow/worker/main.go",
        &request,
    );

    assert!(compact >= UNIQUE_API_SEQUENCE_BONUS);
    assert_eq!(repeated, 0.0);
    assert_eq!(partial, 0.0);
    assert_eq!(
        compact_unique_api_sequence_chunk_bonus(
            8.0,
            &request.query,
            "worker.New RegisterWorkflow RegisterActivity InterruptCh",
            "worker/worker_test.go",
            &request,
        ),
        0.0
    );
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}
