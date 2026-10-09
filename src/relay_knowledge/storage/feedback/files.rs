//! Capability-scoped atomic publication and private filesystem handling.

use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Write},
    path::{Component, Path},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, OpenOptions};

use crate::{
    paths::FeedbackStorePaths,
    ports::feedback_store::{
        FeedbackJournal, FeedbackStoreError, FeedbackStoreErrorKind, MAX_FEEDBACK_JOURNAL_BYTES,
    },
};

use super::{OwnedSemaphorePermit, error, io_error, validation};

pub(super) struct LockedFiles {
    directory: Dir,
    paths: FeedbackStorePaths,
    lock: File,
    _permit: OwnedSemaphorePermit,
}

impl LockedFiles {
    pub(super) fn open(
        paths: FeedbackStorePaths,
        permit: OwnedSemaphorePermit,
    ) -> Result<Self, FeedbackStoreError> {
        let directory = open_private_directory(&paths.directory).map_err(io_error)?;
        let lock =
            open_private_file(&directory, name(&paths.lock)?, false, true).map_err(io_error)?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|failure| {
            if failure.kind() == io::ErrorKind::WouldBlock
                || cfg!(windows) && failure.raw_os_error() == Some(33)
            {
                error(
                    FeedbackStoreErrorKind::Busy,
                    "another feedback transaction is active; retry later",
                )
            } else {
                io_error(failure)
            }
        })?;
        let files = Self {
            directory,
            paths,
            lock,
            _permit: permit,
        };
        // A prepared file has not been acknowledged. Only the atomically
        // installed journal is authoritative after a crash.
        match files.directory.remove_file(name(&files.paths.prepared)?) {
            Ok(()) => {}
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => {}
            Err(failure) => return Err(io_error(failure)),
        }
        Ok(files)
    }

    pub(super) fn read(&self) -> Result<FeedbackJournal, FeedbackStoreError> {
        let file = match open_private_file(&self.directory, name(&self.paths.journal)?, true, false)
        {
            Ok(file) => file,
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => {
                return Ok(FeedbackJournal::default());
            }
            Err(failure) => return Err(io_error(failure)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FEEDBACK_JOURNAL_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > MAX_FEEDBACK_JOURNAL_BYTES {
            return Err(error(
                FeedbackStoreErrorKind::Capacity,
                "feedback journal exceeds the 16 MiB byte budget",
            ));
        }
        let snapshot: FeedbackJournal = serde_json::from_slice(&bytes).map_err(|_| {
            error(
                FeedbackStoreErrorKind::InvalidData,
                "feedback journal is invalid; preserve it for recovery",
            )
        })?;
        validation::validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    pub(super) fn write(&self, snapshot: &FeedbackJournal) -> Result<(), FeedbackStoreError> {
        let mut bytes = LimitedBuffer(Vec::new());
        serde_json::to_writer(&mut bytes, snapshot).map_err(|_| {
            error(
                FeedbackStoreErrorKind::Capacity,
                "feedback journal exceeds the 16 MiB byte budget",
            )
        })?;
        let prepared = name(&self.paths.prepared)?;
        let mut file =
            open_private_file(&self.directory, prepared, false, false).map_err(io_error)?;
        file.write_all(&bytes.0)
            .and_then(|()| file.sync_all())
            .map_err(io_error)?;
        drop(file);
        self.directory
            .rename(prepared, &self.directory, name(&self.paths.journal)?)
            .map_err(io_error)?;
        #[cfg(unix)]
        self.directory
            .open(".")
            .map_err(io_error)?
            .into_std()
            .sync_all()
            .map_err(io_error)?;
        Ok(())
    }
}

impl Drop for LockedFiles {
    fn drop(&mut self) {
        // flock belongs to the open file description: close alone can leave it
        // held by a concurrent fork until that child's close-on-exec runs. Only
        // the last Arc drops this guard, after every commit worker has finished.
        let _ = fs2::FileExt::unlock(&self.lock);
    }
}

fn name(path: &Path) -> Result<&OsStr, FeedbackStoreError> {
    path.file_name().ok_or_else(|| {
        error(
            FeedbackStoreErrorKind::InvalidData,
            "feedback path must have a file name",
        )
    })
}

fn open_private_directory(path: &Path) -> io::Result<Dir> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid feedback directory",
        ));
    }
    let root = path
        .ancestors()
        .last()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing path root"))?;
    let mut directory = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
    for component in path.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        let builder = DirBuilder::new();
        #[cfg(unix)]
        {
            use cap_std::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            match directory.create_dir_with(component, &builder) {
                Ok(()) => directory.open(".")?.into_std().sync_all()?,
                Err(failure) if failure.kind() == io::ErrorKind::AlreadyExists => {}
                Err(failure) => return Err(failure),
            }
        }
        #[cfg(not(unix))]
        match directory.create_dir_with(component, &builder) {
            Ok(()) => {}
            Err(failure) if failure.kind() == io::ErrorKind::AlreadyExists => {}
            Err(failure) => return Err(failure),
        }
        directory = directory.open_dir_nofollow(component)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory
            .open(".")?
            .into_std()
            .set_permissions(std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(directory)
}

fn open_private_file(
    directory: &Dir,
    name: &OsStr,
    read_only: bool,
    lock: bool,
) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    if !read_only {
        options.write(true);
    }
    if lock {
        options.create(true);
    } else if !read_only {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NONBLOCK);
    }
    let file = directory.open_with(name, &options)?.into_std();
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "feedback requires regular files",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "feedback file must not be hard-linked",
            ));
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

struct LimitedBuffer(Vec<u8>);

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_FEEDBACK_JOURNAL_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "feedback byte budget exceeded",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
