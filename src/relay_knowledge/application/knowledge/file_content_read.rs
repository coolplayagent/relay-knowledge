use std::{
    ffi::OsString,
    io::Read,
    path::{Component, Path},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};

pub(super) const MAX_CONTENT_INDEX_BYTES: u64 = 1024 * 1024;

pub(super) fn read_authorized_text_content(
    path: &Path,
    expected: &std::fs::Metadata,
    canonical_root: &Path,
) -> Option<String> {
    let file = open_regular_file_without_following_symlink(path, canonical_root)?;
    let opened = file.metadata().ok()?;
    if !same_file_snapshot(expected, &opened) {
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

fn open_regular_file_without_following_symlink(
    path: &Path,
    canonical_root: &Path,
) -> Option<cap_std::fs::File> {
    let relative_components = relative_content_path_components(path, canonical_root)?;
    let (file_name, directories) = relative_components.split_last()?;
    let mut directory =
        cap_std::fs::Dir::open_ambient_dir(canonical_root, cap_std::ambient_authority()).ok()?;

    for component in directories {
        directory = directory
            .open_dir_nofollow(Path::new(component.as_os_str()))
            .ok()?;
    }

    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    options.follow(FollowSymlinks::No);

    directory
        .open_with(Path::new(file_name.as_os_str()), &options)
        .ok()
}

fn same_file_snapshot(expected: &std::fs::Metadata, opened: &cap_std::fs::Metadata) -> bool {
    opened.is_file()
        && !opened.file_type().is_symlink()
        && opened.len() == expected.len()
        && opened.modified().ok().map(|modified| modified.into_std()) == expected.modified().ok()
}

fn relative_content_path_components(path: &Path, canonical_root: &Path) -> Option<Vec<OsString>> {
    let relative = path.strip_prefix(canonical_root).ok()?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(segment) => components.push(segment.to_owned()),
            Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_)
            | Component::RootDir => return None,
        }
    }
    (!components.is_empty()).then_some(components)
}

#[cfg(test)]
#[path = "file_content_read_tests.rs"]
mod tests;
