use std::{
    collections::VecDeque,
    future::ready,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::{
    domain::{CodeIndexProgressSummary, CodeIndexSummary},
    storage::{CodeIndexFinalizationStep, StorageError},
};

use super::{CompletionMaintenance, drive_session_finalization};

#[tokio::test]
async fn store_driver_advances_multiple_quanta_and_runs_maintenance_after_ready() {
    let expected = summary();
    let steps = Arc::new(Mutex::new(VecDeque::from([
        Ok(CodeIndexFinalizationStep::Pending {
            checkpoint_state: "finalize:files".to_owned(),
        }),
        Ok(CodeIndexFinalizationStep::Pending {
            checkpoint_state: "finalize:symbols".to_owned(),
        }),
        Ok(CodeIndexFinalizationStep::Ready(Box::new(expected.clone()))),
    ])));
    let advance_count = Arc::new(AtomicUsize::new(0));
    let maintenance_count = Arc::new(AtomicUsize::new(0));

    let actual = drive_session_finalization(
        "scope",
        crate::storage::CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let steps = Arc::clone(&steps);
            let advance_count = Arc::clone(&advance_count);
            move || {
                advance_count.fetch_add(1, Ordering::SeqCst);
                ready(
                    steps
                        .lock()
                        .expect("step queue should not be poisoned")
                        .pop_front()
                        .expect("driver should not advance after ready"),
                )
            }
        },
        CompletionMaintenance::BestEffort,
        {
            let maintenance_count = Arc::clone(&maintenance_count);
            move || {
                maintenance_count.fetch_add(1, Ordering::SeqCst);
                ready(())
            }
        },
    )
    .await
    .expect("distinct durable states should reach ready");

    assert_eq!(actual, expected);
    assert_eq!(advance_count.load(Ordering::SeqCst), 3);
    assert_eq!(maintenance_count.load(Ordering::SeqCst), 1);
    assert!(
        steps
            .lock()
            .expect("step queue should not be poisoned")
            .is_empty()
    );
}

#[tokio::test]
async fn fenced_store_driver_returns_ready_without_running_caller_owned_maintenance() {
    let expected = summary();
    let steps = Arc::new(Mutex::new(VecDeque::from([
        Ok(CodeIndexFinalizationStep::Pending {
            checkpoint_state: "finalize:references".to_owned(),
        }),
        Ok(CodeIndexFinalizationStep::Ready(Box::new(expected.clone()))),
    ])));
    let maintenance_count = Arc::new(AtomicUsize::new(0));

    let actual = drive_session_finalization(
        "scope",
        crate::storage::CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let steps = Arc::clone(&steps);
            move || {
                ready(
                    steps
                        .lock()
                        .expect("step queue should not be poisoned")
                        .pop_front()
                        .expect("driver should not advance after ready"),
                )
            }
        },
        CompletionMaintenance::CallerOwned,
        {
            let maintenance_count = Arc::clone(&maintenance_count);
            move || {
                maintenance_count.fetch_add(1, Ordering::SeqCst);
                ready(())
            }
        },
    )
    .await
    .expect("fenced driver should return the ready summary");

    assert_eq!(actual, expected);
    assert_eq!(maintenance_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn store_driver_rejects_a_repeated_durable_checkpoint_state() {
    let advance_count = Arc::new(AtomicUsize::new(0));
    let maintenance_count = Arc::new(AtomicUsize::new(0));

    let error = drive_session_finalization(
        "scope",
        crate::storage::CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let advance_count = Arc::clone(&advance_count);
            move || {
                advance_count.fetch_add(1, Ordering::SeqCst);
                ready(Ok(CodeIndexFinalizationStep::Pending {
                    checkpoint_state: "finalize:stalled".to_owned(),
                }))
            }
        },
        CompletionMaintenance::BestEffort,
        {
            let maintenance_count = Arc::clone(&maintenance_count);
            move || {
                maintenance_count.fetch_add(1, Ordering::SeqCst);
                ready(())
            }
        },
    )
    .await
    .expect_err("a repeated durable state must fail closed");

    assert!(
        matches!(error, StorageError::Invariant(message) if message.contains("did not advance beyond checkpoint state 'finalize:stalled'"))
    );
    assert_eq!(advance_count.load(Ordering::SeqCst), 2);
    assert_eq!(maintenance_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn store_driver_stops_at_the_durable_step_bound_without_maintenance() {
    let advance_count = Arc::new(AtomicUsize::new(0));
    let maintenance_count = Arc::new(AtomicUsize::new(0));

    let error = drive_session_finalization(
        "scope",
        crate::storage::CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let advance_count = Arc::clone(&advance_count);
            move || {
                let step = advance_count.fetch_add(1, Ordering::SeqCst);
                ready(Ok(CodeIndexFinalizationStep::Pending {
                    checkpoint_state: format!("finalize:{step}"),
                }))
            }
        },
        CompletionMaintenance::BestEffort,
        {
            let maintenance_count = Arc::clone(&maintenance_count);
            move || {
                maintenance_count.fetch_add(1, Ordering::SeqCst);
                ready(())
            }
        },
    )
    .await
    .expect_err("an endless advancing plan must stop at the hard bound");

    assert!(
        matches!(error, StorageError::Invariant(message) if message.contains("scope 'scope' exceeded its durable step bound"))
    );
    assert_eq!(
        advance_count.load(Ordering::SeqCst),
        crate::storage::CODE_INDEX_FINALIZATION_MAX_STEPS
    );
    assert_eq!(maintenance_count.load(Ordering::SeqCst), 0);
}

fn summary() -> CodeIndexSummary {
    CodeIndexSummary {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        indexed_file_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_path_count: 0,
        symbol_count: 0,
        handwritten_symbol_count: 0,
        generated_symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        degraded_file_count: 0,
        progress: CodeIndexProgressSummary::default(),
    }
}
