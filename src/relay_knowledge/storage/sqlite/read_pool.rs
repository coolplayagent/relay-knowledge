use std::{
    path::Path,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags};

use crate::storage::StorageError;

const READ_SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(2);
const READ_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(2);
const READ_CONNECTIONS: usize = 4;

#[derive(Debug)]
pub(super) struct ReadConnectionPool {
    connections: Vec<Arc<Mutex<Connection>>>,
    next: AtomicUsize,
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

        Ok(Self {
            connections,
            next: AtomicUsize::new(0),
        })
    }

    pub(super) fn connection(&self) -> Arc<Mutex<Connection>> {
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.connections.len();
        Arc::clone(&self.connections[index])
    }

    pub(super) fn connections(&self) -> Vec<Arc<Mutex<Connection>>> {
        self.connections.clone()
    }
}

pub(super) fn try_lock_any_read_connection(
    connections: &[Arc<Mutex<Connection>>],
) -> Result<std::sync::MutexGuard<'_, Connection>, StorageError> {
    for connection in connections {
        match connection.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => return Err(StorageError::LockPoisoned),
            Err(TryLockError::WouldBlock) => {}
        }
    }

    Err(StorageError::Busy(
        "all sqlite read connections are currently occupied".to_owned(),
    ))
}

pub(super) fn lock_any_read_connection_until<'a>(
    connections: &'a [Arc<Mutex<Connection>>],
    deadline: Instant,
    timeout_message: &str,
) -> Result<std::sync::MutexGuard<'a, Connection>, StorageError> {
    loop {
        for connection in connections {
            match connection.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::Poisoned(_)) => return Err(StorageError::LockPoisoned),
                Err(TryLockError::WouldBlock) => {}
            }
        }
        sleep_until_read_lock_retry(deadline, timeout_message)?;
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
