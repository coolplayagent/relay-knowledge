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
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::{read_authorized_text_content, relative_content_path_components};

    #[test]
    fn relative_content_path_rejects_paths_outside_root() {
        assert!(
            relative_content_path_components(
                Path::new("/archive/docs/wiki.md"),
                Path::new("/workspace")
            )
            .is_none()
        );
    }

    #[test]
    fn relative_content_path_rejects_parent_components() {
        assert!(
            relative_content_path_components(
                Path::new("/workspace/docs/../wiki.md"),
                Path::new("/workspace")
            )
            .is_none()
        );
    }

    #[test]
    fn relative_content_path_accepts_root_child() {
        let components = relative_content_path_components(
            Path::new("/workspace/docs/wiki.md"),
            Path::new("/workspace"),
        )
        .expect("root child should be accepted");

        assert_eq!(components.len(), 2);
    }

    #[test]
    fn read_authorized_text_content_reads_from_capability_root() {
        let fixture = TempContentRoot::new("capability-read");
        let docs = fixture.path().join("docs");
        std::fs::create_dir(&docs).expect("docs directory should be created");
        let file_path = docs.join("wiki.md");
        std::fs::write(&file_path, "service depends on database")
            .expect("content fixture should be written");
        let canonical_root =
            std::fs::canonicalize(fixture.path()).expect("fixture root should canonicalize");
        let canonical_file =
            std::fs::canonicalize(&file_path).expect("fixture file should canonicalize");
        let metadata = std::fs::metadata(&canonical_file).expect("metadata should load");

        let content = read_authorized_text_content(&canonical_file, &metadata, &canonical_root)
            .expect("authorized content should be readable");

        assert_eq!(content, "service depends on database");
    }

    #[cfg(unix)]
    #[test]
    fn read_authorized_text_content_rejects_final_symlink() {
        let fixture = TempContentRoot::new("capability-symlink");
        let target_path = fixture.path().join("target.md");
        let link_path = fixture.path().join("link.md");
        std::fs::write(&target_path, "linked content").expect("target should be written");
        let canonical_target =
            std::fs::canonicalize(&target_path).expect("target should canonicalize");
        std::os::unix::fs::symlink(&canonical_target, &link_path)
            .expect("symlink should be created");
        let canonical_root =
            std::fs::canonicalize(fixture.path()).expect("fixture root should canonicalize");
        let metadata = std::fs::metadata(&link_path).expect("link target metadata should load");

        assert!(read_authorized_text_content(&link_path, &metadata, &canonical_root).is_none());
    }

    struct TempContentRoot {
        path: PathBuf,
    }

    static TEMP_ROOT_COUNTER: AtomicUsize = AtomicUsize::new(0);

    impl TempContentRoot {
        fn new(name: &str) -> Self {
            let counter = TEMP_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = PathBuf::from("target")
                .join("relay-knowledge-test-temp")
                .join(format!(
                    "relay-knowledge-{name}-{}-{counter}",
                    std::process::id(),
                ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("temp root should be created");

            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempContentRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
