//! Cross-process Knowledge Map writer-lock publication and recovery protocol.

use std::{
    fmt::Write as _,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use tokio::time::{Instant, sleep};

use crate::project::{KNOWLEDGE_MAP_FILE_NAME, LEGACY_AGENT_CONTRACT_DIR_NAME};

use super::{KnowledgeMapService, KnowledgeMapServiceError, ensure_owned_directory};

pub(super) const ADVISORY_LOCK_MARKER: &[u8] = b"relay-knowledge advisory writer lock v2\n";
const PREPARED_LOCK_CLEANUP_LIMIT: usize = 64;
const PREPARED_LOCK_RETIREMENT_AGE: Duration = Duration::from_secs(60);
const PREPARED_LOCK_STARTUP_ID_BYTES: usize = 16;
const LOCK_IGNORE_MAX_BYTES: u64 = 64 * 1024;
const LOCK_IGNORE_COMMENT: &[u8] = b"# relay-knowledge transient writer locks\n";
const RETIRED_SHARD_IGNORE: &[u8] = b"/topics/*.retired";
static PREPARED_LOCK_NONCE: AtomicU64 = AtomicU64::new(0);
static PREPARED_LOCK_STARTUP_ID: OnceLock<[u8; PREPARED_LOCK_STARTUP_ID_BYTES]> = OnceLock::new();

impl KnowledgeMapService {
    pub(super) async fn acquire_write_lock(
        &self,
        timeout: Duration,
    ) -> Result<KnowledgeMapWriteLock, KnowledgeMapServiceError> {
        self.acquire_write_lock_at(self.contract_dir_name(), self.map_file_name(), timeout)
            .await
    }

    pub(super) async fn acquire_legacy_write_lock(
        &self,
        timeout: Duration,
    ) -> Result<KnowledgeMapWriteLock, KnowledgeMapServiceError> {
        self.acquire_write_lock_at(
            LEGACY_AGENT_CONTRACT_DIR_NAME,
            KNOWLEDGE_MAP_FILE_NAME,
            timeout,
        )
        .await
    }

    async fn acquire_write_lock_at(
        &self,
        contract_dir_name: &str,
        map_file_name: &str,
        timeout: Duration,
    ) -> Result<KnowledgeMapWriteLock, KnowledgeMapServiceError> {
        let deadline = Instant::now() + timeout;
        let directory = self.repository_root.join(contract_dir_name);
        ensure_owned_directory(&self.repository_root, &directory).await?;
        let ignore_path = directory.join(".gitignore");
        let ignore_worker_path = ignore_path.clone();
        let ignore_canonical = format!("/{map_file_name}.lock").into_bytes();
        let ignore_prepared = format!("/{map_file_name}.lock.prepared.*").into_bytes();
        let ignore_timeout = deadline.saturating_duration_since(Instant::now());
        let ignore_result = tokio::task::spawn_blocking(move || {
            ensure_lock_ignore_contract(
                &ignore_worker_path,
                ignore_timeout,
                &ignore_canonical,
                &ignore_prepared,
                RETIRED_SHARD_IGNORE,
            )
        })
        .await
        .map_err(|error| {
            KnowledgeMapServiceError::Io(std::io::Error::other(format!(
                "knowledge map ignore-contract worker failed: {error}"
            )))
        })?;
        match ignore_result {
            Ok(()) => {}
            Err(error) if lock_is_contended(&error) => {
                return Err(KnowledgeMapServiceError::LockTimeout(ignore_path));
            }
            Err(error) => return Err(KnowledgeMapServiceError::Io(error)),
        }
        let path = directory.join(format!("{map_file_name}.lock"));
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
        let prepared_path = transition_lock_prepared_path(path)?;
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

pub(super) fn transition_lock_prepared_path(path: &Path) -> std::io::Result<PathBuf> {
    let nonce = PREPARED_LOCK_NONCE.fetch_add(1, Ordering::Relaxed);
    let startup_id = prepared_lock_startup_id()?;
    Ok(transition_lock_prepared_path_with_identity(
        path,
        std::process::id(),
        &startup_id,
        nonce,
    ))
}

fn prepared_lock_startup_id() -> std::io::Result<String> {
    if PREPARED_LOCK_STARTUP_ID.get().is_none() {
        let mut candidate = [0_u8; PREPARED_LOCK_STARTUP_ID_BYTES];
        getrandom::getrandom(&mut candidate).map_err(|error| {
            std::io::Error::other(format!("prepared lock startup randomness failed: {error}"))
        })?;
        let _ = PREPARED_LOCK_STARTUP_ID.set(candidate);
    }
    let bytes = PREPARED_LOCK_STARTUP_ID
        .get()
        .ok_or_else(|| std::io::Error::other("prepared lock startup id was not initialized"))?;
    let mut encoded = String::with_capacity(PREPARED_LOCK_STARTUP_ID_BYTES * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}")
            .expect("writing hexadecimal bytes to a String cannot fail");
    }
    Ok(encoded)
}

pub(super) fn transition_lock_prepared_path_with_identity(
    path: &Path,
    process_id: u32,
    startup_id: &str,
    nonce: u64,
) -> PathBuf {
    let mut prepared = path.as_os_str().to_owned();
    prepared.push(format!(".prepared.{process_id}.{startup_id}.{nonce}"));
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
    let mut parts = suffix.split('.');
    let (Some(process), Some(second)) = (parts.next(), parts.next()) else {
        return false;
    };
    if process.is_empty() || !process.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, None) => !second.is_empty() && second.bytes().all(|byte| byte.is_ascii_digit()),
        (Some(nonce), None) => {
            second.len() == PREPARED_LOCK_STARTUP_ID_BYTES * 2
                && second
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && !nonce.is_empty()
                && nonce.bytes().all(|byte| byte.is_ascii_digit())
        }
        _ => false,
    }
}

fn ensure_lock_ignore_contract(
    path: &Path,
    timeout: Duration,
    canonical: &[u8],
    prepared: &[u8],
    retired_shards: &[u8],
) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).append(true).create(true);
    configure_no_follow(&mut options);
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || lock_metadata_is_reparse_point(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "knowledge map lock ignore contract must be a regular file, got {}",
                path.display()
            ),
        ));
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => break,
            Err(error) if lock_is_contended(&error) && std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    }
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut content = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(LOCK_IGNORE_MAX_BYTES + 1)
        .read_to_end(&mut content)?;
    if content.len() as u64 > LOCK_IGNORE_MAX_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "knowledge map lock ignore contract exceeds {LOCK_IGNORE_MAX_BYTES} bytes: {}",
                path.display()
            ),
        ));
    }
    let has_canonical = ignore_contract_has_line(&content, canonical);
    let has_prepared = ignore_contract_has_line(&content, prepared);
    let has_retired_shards = ignore_contract_has_line(&content, retired_shards);
    if has_canonical && has_prepared && has_retired_shards {
        return Ok(());
    }
    let mut addition = Vec::new();
    if content.last().is_some_and(|byte| *byte != b'\n') {
        addition.push(b'\n');
    }
    addition.extend_from_slice(LOCK_IGNORE_COMMENT);
    if !has_canonical {
        addition.extend_from_slice(canonical);
        addition.push(b'\n');
    }
    if !has_prepared {
        addition.extend_from_slice(prepared);
        addition.push(b'\n');
    }
    if !has_retired_shards {
        addition.extend_from_slice(retired_shards);
        addition.push(b'\n');
    }
    file.write_all(&addition)?;
    file.sync_all()
}

fn ignore_contract_has_line(content: &[u8], expected: &[u8]) -> bool {
    content
        .split(|byte| *byte == b'\n')
        .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == expected)
}

fn open_existing_transition_lock(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true);
    configure_no_follow(&mut options);

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

fn configure_no_follow(options: &mut std::fs::OpenOptions) {
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
