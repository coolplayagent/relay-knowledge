use super::*;

#[test]
fn last_guard_releases_lock_even_while_an_inherited_descriptor_remains_open() {
    use crate::{
        env::{EnvironmentConfig, PlatformKind},
        paths::RuntimePaths,
    };
    use std::sync::Arc;
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "relay-feedback-inherited-lock-{}",
        std::process::id(),
    ));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.as_os_str())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths)
        .unwrap()
        .feedback_store_paths();
    let permit = Arc::new(tokio::sync::Semaphore::new(1))
        .try_acquire_owned()
        .unwrap();
    let files = Arc::new(LockedFiles::open(paths.clone(), permit).unwrap());
    // A safe duplicate has the same Unix open-file-description semantics as
    // an inherited descriptor during another thread's fork/exec boundary.
    let inherited = files.lock.try_clone().unwrap();
    let contender = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&paths.lock)
        .unwrap();
    let active_worker = files.clone();
    drop(files);
    assert!(fs2::FileExt::try_lock_exclusive(&contender).is_err());
    drop(active_worker);
    fs2::FileExt::try_lock_exclusive(&contender)
        .expect("the final owner explicitly releases its lock");
    assert!(
        inherited.metadata().is_ok(),
        "the duplicated descriptor remains alive"
    );
    fs2::FileExt::unlock(&contender).unwrap();
    drop(contender);
    drop(inherited);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn encoding_never_allocates_beyond_the_journal_budget() {
    let mut buffer = LimitedBuffer(Vec::new());
    buffer
        .write_all(&vec![b'x'; MAX_FEEDBACK_JOURNAL_BYTES])
        .unwrap();
    assert!(buffer.write_all(b"x").is_err());
    assert_eq!(buffer.0.len(), MAX_FEEDBACK_JOURNAL_BYTES);
    buffer.flush().unwrap();
}
