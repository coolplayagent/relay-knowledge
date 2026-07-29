use std::{
    path::Path,
    sync::{Arc, Mutex, TryLockError},
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags};

use crate::storage::StorageError;

use super::maintenance::configure_read_connection;

const READ_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(2);
const READ_CONNECTIONS: usize = 4;

#[derive(Debug)]
pub(in crate::storage::sqlite) struct ReadConnectionPool {
    connections: Vec<Arc<Mutex<Connection>>>,
}

impl ReadConnectionPool {
    pub(in crate::storage::sqlite) fn open(path: &Path) -> Result<Self, StorageError> {
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

    pub(in crate::storage::sqlite) fn connections(&self) -> Vec<Arc<Mutex<Connection>>> {
        self.connections.clone()
    }
}

pub(in crate::storage::sqlite) fn try_lock_any_read_connection(
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

pub(in crate::storage::sqlite) fn lock_any_read_connection(
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

pub(in crate::storage::sqlite) fn lock_any_read_connection_until<'a>(
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

pub(in crate::storage::sqlite) fn lock_connection_until<'a>(
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

#[cfg(test)]
#[path = "read_pool_tests.rs"]
mod tests;
