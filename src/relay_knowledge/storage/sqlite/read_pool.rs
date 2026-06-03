use std::{
    path::Path,
    sync::{Arc, Mutex, TryLockError},
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags};

use crate::storage::StorageError;

const READ_SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(50);
const READ_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(2);
const READ_CONNECTIONS: usize = 4;

#[derive(Debug)]
pub(super) struct ReadConnectionPool {
    connections: Vec<Arc<Mutex<Connection>>>,
}

impl ReadConnectionPool {
    pub(super) fn open(path: &Path) -> Result<Self, StorageError> {
        let mut connections = Vec::with_capacity(READ_CONNECTIONS);
        for _ in 0..READ_CONNECTIONS {
            let connection = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            configure_read_connection(&connection)?;
            connections.push(Arc::new(Mutex::new(connection)));
        }

        Ok(Self { connections })
    }

    pub(super) fn connections(&self) -> Vec<Arc<Mutex<Connection>>> {
        self.connections.clone()
    }
}

pub(super) fn try_lock_any_read_connection(
    connections: &[Arc<Mutex<Connection>>],
) -> Result<std::sync::MutexGuard<'_, Connection>, StorageError> {
    let mut saw_busy_connection = false;
    let mut saw_poisoned_connection = false;
    for connection in connections {
        match connection.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => saw_poisoned_connection = true,
            Err(TryLockError::WouldBlock) => saw_busy_connection = true,
        }
    }

    if saw_busy_connection {
        return Err(StorageError::Busy(
            "all healthy sqlite read connections are currently occupied".to_owned(),
        ));
    }
    if saw_poisoned_connection {
        return Err(StorageError::LockPoisoned);
    }

    Err(StorageError::Busy(
        "sqlite read pool has no connections".to_owned(),
    ))
}

pub(super) fn lock_any_read_connection(
    connections: &[Arc<Mutex<Connection>>],
) -> Result<std::sync::MutexGuard<'_, Connection>, StorageError> {
    loop {
        match try_lock_any_read_connection(connections) {
            Ok(guard) => return Ok(guard),
            Err(StorageError::Busy(_)) => std::thread::sleep(READ_LOCK_POLL_INTERVAL),
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn lock_any_read_connection_until<'a>(
    connections: &'a [Arc<Mutex<Connection>>],
    deadline: Instant,
    timeout_message: &str,
) -> Result<std::sync::MutexGuard<'a, Connection>, StorageError> {
    loop {
        match try_lock_any_read_connection(connections) {
            Ok(guard) => return Ok(guard),
            Err(StorageError::Busy(_)) => sleep_until_read_lock_retry(deadline, timeout_message)?,
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn lock_connection_until<'a>(
    connection: &'a Arc<Mutex<Connection>>,
    deadline: Instant,
    timeout_message: &str,
) -> Result<std::sync::MutexGuard<'a, Connection>, StorageError> {
    loop {
        match connection.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => return Err(StorageError::LockPoisoned),
            Err(TryLockError::WouldBlock) => {
                sleep_until_read_lock_retry(deadline, timeout_message)?;
            }
        }
    }
}

fn sleep_until_read_lock_retry(
    deadline: Instant,
    timeout_message: &str,
) -> Result<(), StorageError> {
    let now = Instant::now();
    if now >= deadline {
        return Err(StorageError::InvalidInput(timeout_message.to_owned()));
    }
    let remaining = deadline.saturating_duration_since(now);
    std::thread::sleep(remaining.min(READ_LOCK_POLL_INTERVAL));

    Ok(())
}

fn configure_read_connection(connection: &Connection) -> Result<(), StorageError> {
    connection.busy_timeout(READ_SQLITE_BUSY_TIMEOUT)?;
    connection.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA query_only = ON;
        ",
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
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
}
