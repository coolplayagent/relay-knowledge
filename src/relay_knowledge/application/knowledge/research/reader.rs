//! Capability-confined, byte-preserving reads behind bounded cancellable workers.

use crate::{
    api::ApiError,
    domain::research::{MAX_RESEARCH_JSON_BYTES, ResearchArtifact, ResearchPathBase},
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_JOB_BYTES: usize = 256 * 1024 * 1024;
static PERMITS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(4)));

pub(super) struct ResearchReader {
    directory: cap_std::fs::Dir,
    remaining_bytes: usize,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(super) async fn run<T, F>(root: PathBuf, operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&mut ResearchReader) -> Result<T, ApiError> + Send + 'static,
{
    compute(move |cancelled| {
        let directory = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority())
            .map_err(|error| ApiError::invalid_argument(format!("research root: {error}")))?;
        let mut reader = ResearchReader {
            directory,
            remaining_bytes: MAX_JOB_BYTES,
            deadline: Instant::now() + Duration::from_secs(30),
            cancelled,
        };
        operation(&mut reader)
    })
    .await
}

pub(super) async fn compute<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(Arc<AtomicBool>) -> Result<T, ApiError> + Send + 'static,
{
    let permit = Arc::clone(&PERMITS)
        .try_acquire_owned()
        .map_err(|_| ApiError::qos_rejected("research worker budget exhausted"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let _guard = CancelOnDrop(Arc::clone(&cancelled));
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation(cancelled)
    });
    tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .map_err(|_| ApiError::storage_unavailable("research audit deadline exceeded"))?
        .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
}

impl ResearchReader {
    pub(super) fn json<T: DeserializeOwned>(
        &mut self,
        path: &Path,
    ) -> Result<(T, String), ApiError> {
        let bytes = self
            .read(path, MAX_RESEARCH_JSON_BYTES)
            .map_err(ApiError::invalid_argument)?;
        let value = serde_json::from_slice(&bytes).map_err(|error| {
            ApiError::invalid_argument(format!("invalid research JSON: {error}"))
        })?;
        Ok((value, digest(&bytes)))
    }

    pub(super) fn artifact(
        &mut self,
        artifact: &ResearchArtifact,
        input: &Path,
    ) -> Result<Vec<u8>, String> {
        let path = artifact_path(artifact, input)?;
        self.read(&path, MAX_FILE_BYTES)
    }

    pub(super) fn read(&mut self, path: &Path, limit: usize) -> Result<Vec<u8>, String> {
        self.check_budget()?;
        let components = confined_components(path)?;
        let (name, parents) = components.split_last().ok_or("empty artifact path")?;
        let mut directory = self
            .directory
            .try_clone()
            .map_err(|error| error.to_string())?;
        for parent in parents {
            directory = directory
                .open_dir_nofollow(Path::new(parent))
                .map_err(|error| format!("unsafe or unreadable parent: {error}"))?;
        }
        let metadata = directory
            .symlink_metadata(Path::new(name))
            .map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("artifact must be a regular file without symlinks".into());
        }
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let mut file = directory
            .open_with(Path::new(name), &options)
            .map_err(|error| error.to_string())?;
        let opened = file.metadata().map_err(|error| error.to_string())?;
        if !opened.is_file()
            || opened.len() > limit as u64
            || opened.len() > self.remaining_bytes as u64
        {
            return Err("artifact exceeds file budget or is not regular".into());
        }
        let mut bytes = Vec::new();
        let mut buffer = [0; 65536];
        loop {
            self.check_budget()?;
            let read_limit = buffer.len().min(self.remaining_bytes.saturating_add(1));
            let count = file
                .read(&mut buffer[..read_limit])
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            self.remaining_bytes = self
                .remaining_bytes
                .checked_sub(count)
                .ok_or("research byte budget exceeded")?;
            if bytes.len() + count > limit {
                return Err("artifact exceeds file byte budget".into());
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        let after = file.metadata().map_err(|error| error.to_string())?;
        if after.len() != opened.len()
            || after.modified().ok() != opened.modified().ok()
            || bytes.len() as u64 != after.len()
        {
            return Err("artifact changed while reading; retry audit".into());
        }
        Ok(bytes)
    }

    fn check_budget(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) || Instant::now() >= self.deadline {
            return Err("research read cancelled or resource budget exhausted".into());
        }
        Ok(())
    }
}

/// Validate supplied POSIX components before converting a platform-native join back
/// to the portable artifact representation (Windows joins introduce backslashes).
pub(super) fn artifact_path(artifact: &ResearchArtifact, input: &Path) -> Result<PathBuf, String> {
    confined_components(Path::new(&artifact.path))?;
    let path = match artifact.path_base {
        ResearchPathBase::Repository => PathBuf::from(&artifact.path),
        ResearchPathBase::Catalog => {
            confined_components(input)?;
            input.parent().unwrap_or(Path::new("")).join(&artifact.path)
        }
    };
    let text = path.to_str().ok_or("artifact path must be UTF-8")?;
    Ok(PathBuf::from(text.replace('\\', "/")))
}

fn confined_components(path: &Path) -> Result<Vec<std::ffi::OsString>, String> {
    if path.as_os_str().to_string_lossy().contains(['\\', ':']) {
        return Err("artifact path must be a confined POSIX relative path".into());
    }
    path.components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name.to_os_string()),
            _ => Err("artifact path must not be absolute or contain traversal".into()),
        })
        .collect()
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
