use super::*;

#[test]
fn detects_new_file_as_changed() {
    let mut cache = ContentHashCache::new(100);
    assert!(
        cache
            .check_and_update(PathBuf::from("a.rs"), b"hello")
            .changed
    );
}

#[test]
fn detects_unchanged_file_as_not_changed() {
    let mut cache = ContentHashCache::new(100);
    cache.check_and_update(PathBuf::from("a.rs"), b"hello");
    assert!(
        !cache
            .check_and_update(PathBuf::from("a.rs"), b"hello")
            .changed
    );
}

#[test]
fn detects_modified_file_as_changed() {
    let mut cache = ContentHashCache::new(100);
    cache.check_and_update(PathBuf::from("a.rs"), b"hello");
    assert!(
        cache
            .check_and_update(PathBuf::from("a.rs"), b"world")
            .changed
    );
}

#[test]
fn returns_stable_content_hash_for_same_bytes() {
    let mut cache = ContentHashCache::new(100);
    let first = cache.check_and_update(PathBuf::from("a.rs"), b"hello");
    let second = cache.check_and_update(PathBuf::from("a.rs"), b"hello");
    assert_eq!(first.hash, second.hash);
    assert!(!second.changed);
}

#[test]
fn evicts_when_at_capacity() {
    let mut cache = ContentHashCache::new(2);
    cache.check_and_update(PathBuf::from("a.rs"), b"a");
    cache.check_and_update(PathBuf::from("b.rs"), b"b");
    cache.check_and_update(PathBuf::from("c.rs"), b"c");
    assert_eq!(cache.len(), 2);
}

#[test]
fn remove_clears_entry() {
    let mut cache = ContentHashCache::new(100);
    cache.check_and_update(PathBuf::from("a.rs"), b"a");
    let key = PathBuf::from("a.rs");
    cache.remove(&key);
    assert!(cache.is_empty());
}

#[test]
fn clear_empties_all_entries() {
    let mut cache = ContentHashCache::new(100);
    cache.check_and_update(PathBuf::from("a.rs"), b"a");
    cache.check_and_update(PathBuf::from("b.rs"), b"b");
    cache.clear();
    assert!(cache.is_empty());
}

#[test]
fn update_at_capacity_replaces_existing_without_eviction() {
    let mut cache = ContentHashCache::new(2);
    cache.check_and_update(PathBuf::from("a.rs"), b"a");
    cache.check_and_update(PathBuf::from("b.rs"), b"b");
    assert!(!cache.check_and_update(PathBuf::from("a.rs"), b"a").changed);
    assert_eq!(cache.len(), 2);
}

#[test]
fn evicts_in_insertion_order() {
    let mut cache = ContentHashCache::new(2);
    cache.check_and_update(PathBuf::from("a.rs"), b"a");
    cache.check_and_update(PathBuf::from("b.rs"), b"b");
    cache.check_and_update(PathBuf::from("c.rs"), b"c");
    assert_eq!(cache.len(), 2);
    assert!(cache.check_and_update(PathBuf::from("a.rs"), b"a").changed);
    assert_eq!(cache.len(), 2);
}

#[test]
fn zero_capacity_tracks_no_entries() {
    let mut cache = ContentHashCache::new(0);
    let observation = cache.check_and_update(PathBuf::from("a.rs"), b"a");
    assert!(observation.changed);
    assert_ne!(observation.hash, 0);
    assert!(cache.is_empty());
}
