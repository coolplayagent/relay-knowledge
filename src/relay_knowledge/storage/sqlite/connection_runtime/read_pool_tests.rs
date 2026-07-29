use super::*;

#[test]
fn try_lock_any_read_connection_skips_poisoned_lane() {
    let poisoned = memory_connection();
    poison_connection(&poisoned);
    let connections = vec![poisoned, memory_connection()];

    let guard =
        try_lock_any_read_connection(&connections).expect("healthy lane should be selected");

    assert!(guard.is_autocommit());
}

#[test]
fn try_lock_any_read_connection_reports_busy_when_healthy_lanes_are_busy() {
    let poisoned = memory_connection();
    poison_connection(&poisoned);
    let connections = vec![poisoned, memory_connection()];
    let _held = connections[1].lock().expect("healthy lane should lock");

    let error = match try_lock_any_read_connection(&connections) {
        Ok(_) => panic!("busy healthy lane should not be masked by poisoned lane"),
        Err(error) => error,
    };

    assert!(matches!(error, StorageError::Busy(message) if message.contains("occupied")));
}

#[test]
fn try_lock_any_read_connection_reports_poisoned_when_all_lanes_are_poisoned() {
    let first = memory_connection();
    let second = memory_connection();
    poison_connection(&first);
    poison_connection(&second);
    let connections = vec![first, second];

    let error = match try_lock_any_read_connection(&connections) {
        Ok(_) => panic!("all poisoned lanes should fail explicitly"),
        Err(error) => error,
    };

    assert!(matches!(error, StorageError::LockPoisoned));
}

#[test]
fn lock_any_read_connection_until_skips_poisoned_lane() {
    let poisoned = memory_connection();
    poison_connection(&poisoned);
    let healthy = memory_connection();
    let connections = vec![poisoned, healthy];
    let deadline = Instant::now() + Duration::from_millis(50);

    let guard = lock_any_read_connection_until(&connections, deadline, "read lock timed out")
        .expect("healthy lane should be selected");

    assert!(guard.is_autocommit());
}

fn memory_connection() -> Arc<Mutex<Connection>> {
    Arc::new(Mutex::new(
        Connection::open_in_memory().expect("memory connection should open"),
    ))
}

fn poison_connection(connection: &Arc<Mutex<Connection>>) {
    let connection = Arc::clone(connection);
    let result = std::thread::spawn(move || {
        let _guard = connection
            .lock()
            .expect("connection should lock before panic");
        panic!("poison read lane");
    })
    .join();
    assert!(result.is_err());
}
