//! Cross-process Knowledge Map writer-lock publication and recovery protocol.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use tokio::time::{Instant, sleep};

use super::{KnowledgeMapService, KnowledgeMapServiceError, ensure_owned_directory};
use crate::project::{AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_FILE_NAME};

pub(super) const ADVISORY_LOCK_MARKER: &[u8] = b"relay-knowledge advisory writer lock v2\n";
const PREPARED_LOCK_CLEANUP_LIMIT: usize = 64;
const PREPARED_LOCK_RETIREMENT_AGE: Duration = Duration::from_secs(60);
static PREPARED_LOCK_NONCE: AtomicU64 = AtomicU64::new(0);

impl KnowledgeMapService {
    pub(super) async fn acquire_write_lock(
        &self,
        timeout: Duration,
    ) -> Result<KnowledgeMapWriteLock, KnowledgeMapServiceError> {
        let directory = self.repository_root.join(AGENT_CONTRACT_DIR_NAME);
        ensure_owned_directory(&self.repository_root, &directory).await?;
        let path = directory.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
        let deadline = Instant::now() + timeout;
        loop {
            let open_path = path.clone();
            let candidate = tokio::task::spawn_blocking(move || open_transition_lock(&open_path))
                .await
                .map_err(|error| {
                    KnowledgeMapServiceError::Io(std::io::Error::other(format!(
                        "knowledge map lock worker failed: {error}"
                    )))
                })?;
            let candidate = match candidate {
                Ok(candidate) => candidate,
                Err(error) if lock_is_contended(&error) => {
                    if Instant::now() >= deadline {
                        return Err(KnowledgeMapServiceError::LockTimeout(path));
                    }
                    sleep(Duration::from_millis(25)).await;
                    continue;
                }
                Err(error) => return Err(KnowledgeMapServiceError::Io(error)),
            };
            let file = match candidate {
                TransitionLock::AdvisoryOwned(file) => {
                    return Ok(KnowledgeMapWriteLock { file });
                }
                TransitionLock::Advisory(file) => file,
                TransitionLock::Legacy => {
                    if Instant::now() >= deadline {
                        return Err(KnowledgeMapServiceError::LockTimeout(path));
                    }
                    sleep(Duration::from_millis(25)).await;
                    continue;
                }
            };
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(KnowledgeMapWriteLock { file }),
                Err(error) if lock_is_contended(&error) => {
                    if Instant::now() >= deadline {
                        return Err(KnowledgeMapServiceError::LockTimeout(path));
                    }
                    sleep(Duration::from_millis(25)).await;
                }
                Err(error) => return Err(KnowledgeMapServiceError::Io(error)),
            }
        }
    }
}

enum TransitionLock {
    Advisory(std::fs::File),
    AdvisoryOwned(std::fs::File),
    Legacy,
}

fn open_transition_lock(path: &Path) -> std::io::Result<TransitionLock> {
    cleanup_transition_locks(path, PREPARED_LOCK_RETIREMENT_AGE);
    match open_existing_transition_lock(path) {
        Ok(file) => classify_transition_lock(file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            publish_marked_transition_lock(path)
        }
        Err(error) => Err(error),
    }
}

fn classify_transition_lock(mut file: std::fs::File) -> std::io::Result<TransitionLock> {
    use std::io::Read;

    let mut marker = Vec::with_capacity(ADVISORY_LOCK_MARKER.len() + 1);
    std::io::Read::by_ref(&mut file)
        .take((ADVISORY_LOCK_MARKER.len() + 1) as u64)
        .read_to_end(&mut marker)?;
    if marker == ADVISORY_LOCK_MARKER {
        Ok(TransitionLock::Advisory(file))
    } else {
        Ok(TransitionLock::Legacy)
    }
}

fn publish_marked_transition_lock(path: &Path) -> std::io::Result<TransitionLock> {
    use std::io::Write;

    let (prepared_path, mut file) = create_transition_lock_staging(path)?;
    fs2::FileExt::try_lock_exclusive(&file)?;
    if let Err(error) = file
        .write_all(ADVISORY_LOCK_MARKER)
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(prepared_path);
        return Err(error);
    }
    match std::fs::hard_link(&prepared_path, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(prepared_path);
            Ok(TransitionLock::AdvisoryOwned(file))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            drop(file);
            let _ = std::fs::remove_file(prepared_path);
            match open_existing_transition_lock(path) {
                Ok(file) => classify_transition_lock(file),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
                }
                Err(error) => Err(error),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            drop(file);
            Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
        }
        Err(error) => {
            drop(file);
            let _ = std::fs::remove_file(prepared_path);
            Err(error)
        }
    }
}

fn create_transition_lock_staging(path: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    for _ in 0..16 {
        let prepared_path = transition_lock_prepared_path(path);
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&prepared_path)
        {
            Ok(file) => return Ok((prepared_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique prepared knowledge map writer lock",
    ))
}

pub(super) fn transition_lock_prepared_path(path: &Path) -> PathBuf {
    let nonce = PREPARED_LOCK_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut prepared = path.as_os_str().to_owned();
    prepared.push(format!(".prepared.{}.{nonce}", std::process::id()));
    PathBuf::from(prepared)
}

pub(super) fn cleanup_transition_locks(path: &Path, retirement_age: Duration) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(lock_name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let prefix = format!("{lock_name}.prepared.");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten().take(PREPARED_LOCK_CLEANUP_LIMIT) {
        let name = entry.file_name();
        let candidate = name
            .to_str()
            .and_then(|name| name.strip_prefix(&prefix))
            .is_some_and(valid_prepared_lock_suffix);
        if !candidate {
            continue;
        }
        let candidate_path = entry.path();
        let Ok(file) = open_existing_transition_lock(&candidate_path) else {
            continue;
        };
        let retired = file
            .metadata()
            .and_then(|metadata| metadata.modified())
            .and_then(|modified| {
                SystemTime::now()
                    .duration_since(modified)
                    .map_err(std::io::Error::other)
            })
            .is_ok_and(|age| age >= retirement_age);
        if retired && fs2::FileExt::try_lock_exclusive(&file).is_ok() {
            let _ = std::fs::remove_file(candidate_path);
        }
    }
}

fn valid_prepared_lock_suffix(suffix: &str) -> bool {
    let Some((process, nonce)) = suffix.split_once('.') else {
        return false;
    };
    !process.is_empty()
        && !nonce.is_empty()
        && process.bytes().all(|byte| byte.is_ascii_digit())
        && nonce.bytes().all(|byte| byte.is_ascii_digit())
}

fn open_existing_transition_lock(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || lock_metadata_is_reparse_point(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "knowledge map writer lock must be a regular file, got {}",
                path.display()
            ),
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn lock_metadata_is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn lock_metadata_is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn lock_is_contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || cfg!(windows) && error.raw_os_error() == Some(33)
}

#[derive(Debug)]
pub(super) struct KnowledgeMapWriteLock {
    file: std::fs::File,
}

impl Drop for KnowledgeMapWriteLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
