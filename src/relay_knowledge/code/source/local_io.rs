//! Local source I/O boundaries with bounded directory materialization.

#[cfg(test)]
use crate::domain::CodePathIoOperation;
use std::{fs, io, path::Path};

// Bounds one directory independently of total admitted index paths.
const MAX_DIRECTORY_ENTRIES: usize = 100_000;

pub(in crate::code) fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(test)]
    test_fault::check(path, CodePathIoOperation::Read)?;
    fs::read(path)
}

pub(in crate::code) fn symlink_metadata(path: &Path) -> io::Result<fs::Metadata> {
    #[cfg(test)]
    test_fault::check(path, CodePathIoOperation::Metadata)?;
    fs::symlink_metadata(path)
}

/// Type lookup can perform I/O on filesystems whose directory entries omit type evidence.
pub(in crate::code) fn directory_entry_type(entry: &fs::DirEntry) -> io::Result<fs::FileType> {
    #[cfg(test)]
    test_fault::check_entry_type(&entry.path())?;
    entry.file_type()
}

/// A failed iterator returns no partial listing to its caller.
pub(in crate::code) fn read_directory(path: &Path) -> io::Result<Vec<fs::DirEntry>> {
    #[cfg(test)]
    test_fault::check(path, CodePathIoOperation::ReadDirectory)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        #[cfg(test)]
        test_fault::check(path, CodePathIoOperation::ReadDirectory)?;
        if entries.len() == MAX_DIRECTORY_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "local directory exceeds bounded entry budget",
            ));
        }
        entries.push(entry?);
    }
    Ok(entries)
}

#[cfg(test)]
#[path = "local_io_fault.rs"]
pub(in crate::code) mod test_fault;

#[cfg(test)]
#[path = "local_io_tests.rs"]
mod tests;
