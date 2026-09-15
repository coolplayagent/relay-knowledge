use super::*;
use crate::paths::StorageDirectoryAccess::{ExistingOnly, OpenOrCreate};

static SECURITY_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn fresh_database_policy_recovers_control_and_shard_accounts_without_filesystem_work() {
    for path in [
        "D:/relay-knowledge/users/S-1-5-21-1-2-3-1001/data/relay-knowledge.sqlite",
        "D:/relay-knowledge/users/S-1-5-21-1-2-3-1001/data/backends/repositories/repo/code.sqlite",
    ] {
        for access in [ExistingOnly, OpenOrCreate] {
            assert!(
                managed_database_validation(Path::new(path), access)
                    .unwrap()
                    .is_some()
            );
        }
    }
    let custom = Path::new("E:/custom/relay-knowledge.sqlite");
    if managed_database_validation(custom, ExistingOnly)
        .unwrap()
        .is_none()
    {
        validate_new_database_access(custom, ExistingOnly).unwrap();
    }
    for access in [ExistingOnly, OpenOrCreate] {
        assert!(
            validate_new_database_access(
                Path::new("D:/relay-knowledge/users/S-1-invalid/data/relay-knowledge.sqlite"),
                access
            )
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
        );
    }
    assert!(
        validate_new_database_access(Path::new(&"x".repeat(4097)), ExistingOnly)
            .unwrap_err()
            .to_string()
            .contains("4096")
    );
}

#[test]
fn fresh_managed_database_open_checks_policy_with_or_without_an_ambient_runtime() {
    let _serial = SECURITY_TEST.lock().unwrap();
    let missing = Path::new(
        "D:/relay-knowledge/users/S-1-5-21-4294967295-4294967295-4294967295-4294967295/data/review-missing.sqlite",
    );
    let error = validate_new_database_access(missing, ExistingOnly).unwrap_err();
    assert!(!error.to_string().contains("no reactor"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(async { validate_new_database_access(missing, ExistingOnly) })
        .unwrap_err();
    assert!(!error.to_string().contains("no reactor"));
    assert!(
        !missing.exists(),
        "existing-only checks must never create a database"
    );
}

#[test]
fn concurrent_security_admission_waits_for_the_active_worker_and_releases_capacity() {
    let gate = SecurityGate {
        state: Mutex::new(AdmissionState {
            active: false,
            waiting: 0,
        }),
        available: Condvar::new(),
    };
    let active = gate.acquire(Duration::from_secs(1)).unwrap();
    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker_gate = &gate;
        scope.spawn(move || {
            ready_tx.send(()).unwrap();
            let permit = worker_gate.acquire(Duration::from_secs(2));
            result_tx.send(permit.is_ok()).unwrap();
        });
        ready_rx.recv().unwrap();
        assert!(matches!(
            result_rx.recv_timeout(Duration::from_millis(20)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(active);
        assert!(result_rx.recv_timeout(Duration::from_secs(2)).unwrap());
    });
    assert_eq!(gate.state.lock().unwrap().waiting, 0);
    gate.acquire(Duration::ZERO).unwrap();
}

#[test]
fn security_admission_bounds_waiters_and_wait_time() {
    let gate = SecurityGate {
        state: Mutex::new(AdmissionState {
            active: true,
            waiting: MAX_SECURITY_WAITERS,
        }),
        available: Condvar::new(),
    };
    assert!(
        matches!(gate.acquire(Duration::ZERO), Err(StorageError::Busy(message)) if message.contains("queue is full"))
    );
    gate.state.lock().unwrap().waiting = 0;
    assert!(
        matches!(gate.acquire(Duration::from_millis(1)), Err(StorageError::Busy(message)) if message.contains("timed out"))
    );
    assert_eq!(gate.state.lock().unwrap().waiting, 0);
    gate.state.lock().unwrap().active = false;
    gate.acquire(Duration::ZERO).unwrap();
}
