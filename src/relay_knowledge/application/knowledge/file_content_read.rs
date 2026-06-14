use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub(super) const MAX_CONTENT_INDEX_BYTES: u64 = 1024 * 1024;

pub(super) fn read_authorized_text_content(
    path: &Path,
    expected: &std::fs::Metadata,
    canonical_root: &Path,
) -> Option<String> {
    let file = open_regular_file_without_following_symlink(path)?;
    let opened = file.metadata().ok()?;
    if !same_file_snapshot(expected, &opened) {
        return None;
    }
    if !opened_file_stays_under_root(&file, path, canonical_root) {
        return None;
    }
    let mut reader = file.take(MAX_CONTENT_INDEX_BYTES.saturating_add(1));
    let mut content = String::new();
    reader.read_to_string(&mut content).ok()?;
    if u64::try_from(content.len()).ok()? > MAX_CONTENT_INDEX_BYTES {
        return None;
    }
    Some(content)
}

#[cfg(unix)]
fn open_regular_file_without_following_symlink(path: &Path) -> Option<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()
}

#[cfg(windows)]
fn open_regular_file_without_following_symlink(path: &Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()
}

#[cfg(not(any(unix, windows)))]
fn open_regular_file_without_following_symlink(_path: &Path) -> Option<std::fs::File> {
    None
}

fn same_file_snapshot(expected: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    opened.is_file()
        && !opened.file_type().is_symlink()
        && opened.len() == expected.len()
        && opened.modified().ok() == expected.modified().ok()
}

fn opened_file_stays_under_root(file: &std::fs::File, path: &Path, canonical_root: &Path) -> bool {
    opened_file_path(file)
        .or_else(|| std::fs::canonicalize(path).ok())
        .is_some_and(|opened_path| opened_path.starts_with(canonical_root))
}

#[cfg(target_os = "linux")]
fn opened_file_path(file: &std::fs::File) -> Option<PathBuf> {
    use std::os::fd::AsRawFd;

    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()
}

#[cfg(not(target_os = "linux"))]
fn opened_file_path(_file: &std::fs::File) -> Option<PathBuf> {
    None
}
