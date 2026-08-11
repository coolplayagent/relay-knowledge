use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use tokio::sync::Semaphore;

use crate::api::ErrorKind;

use super::{run_blocking_domain, run_blocking_domain_with_policy};

#[tokio::test(flavor = "current_thread")]
async fn domain_projection_runs_off_executor_and_maps_domain_errors() {
    let runtime_thread = thread::current().id();
    let blocking_thread =
        run_blocking_domain(|| Ok::<_, crate::domain::DomainError>(thread::current().id()))
            .await
            .expect("bounded domain projection should complete");
    assert_ne!(blocking_thread, runtime_thread);

    let error = run_blocking_domain(|| {
        Err::<(), _>(crate::domain::DomainError::invalid(
            "focus_path",
            "must identify a concept",
        ))
    })
    .await
    .expect_err("domain validation should cross the blocking boundary");
    assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    assert!(error.message.contains("focus_path"));
}

#[tokio::test(flavor = "current_thread")]
async fn response_timeout_keeps_permit_until_worker_finishes_then_recovers() {
    let permits = Arc::new(Semaphore::new(1));
    let (release_sender, release_receiver) = mpsc::channel();
    let timeout = run_blocking_domain_with_policy(
        move || {
            release_receiver.recv().expect("worker should be released");
            Ok::<_, crate::domain::DomainError>(())
        },
        Arc::clone(&permits),
        Duration::from_secs(1),
        Duration::from_millis(10),
    )
    .await
    .expect_err("response wait should time out");
    assert_eq!(timeout.error_kind, ErrorKind::Timeout);
    assert_eq!(permits.available_permits(), 0);

    let saturated = run_blocking_domain_with_policy(
        || Ok::<_, crate::domain::DomainError>(()),
        Arc::clone(&permits),
        Duration::from_millis(10),
        Duration::from_secs(1),
    )
    .await
    .expect_err("held worker permit should bound queued work");
    assert_eq!(saturated.error_kind, ErrorKind::QosRejected);

    release_sender
        .send(())
        .expect("worker should still be running");
    let recovered = run_blocking_domain_with_policy(
        || Ok::<_, crate::domain::DomainError>(42_usize),
        permits,
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .await
    .expect("permit should return after the timed-out worker finishes");
    assert_eq!(recovered, 42);
}
