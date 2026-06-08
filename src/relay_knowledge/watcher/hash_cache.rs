use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ContentHashCache {
    capacity: usize,
    entries: HashMap<PathBuf, u64>,
    insertion_order: VecDeque<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentHashObservation {
    pub changed: bool,
    pub hash: u64,
}

impl ContentHashCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
        }
    }

    pub fn check_and_update(&mut self, path: PathBuf, content: &[u8]) -> ContentHashObservation {
        self.check_hash_and_update(path, content_hash64(content))
    }

    pub fn check_hash_and_update(
        &mut self,
        path: PathBuf,
        new_hash: u64,
    ) -> ContentHashObservation {
        let observation = self.observe_hash(&path, new_hash);
        if observation.changed {
            self.record_hash(path, new_hash);
        }
        observation
    }

    pub fn observe_hash(&self, path: &PathBuf, new_hash: u64) -> ContentHashObservation {
        if self.capacity == 0 {
            return ContentHashObservation {
                changed: true,
                hash: new_hash,
            };
        }
        let changed = match self.entries.get(path) {
            Some(&existing) => existing != new_hash,
            None => true,
        };
        ContentHashObservation {
            changed,
            hash: new_hash,
        }
    }

    pub fn record_hash(&mut self, path: PathBuf, new_hash: u64) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&path) {
            self.evict_oldest();
        }
        if !self.entries.contains_key(&path) {
            self.insertion_order.push_back(path.clone());
        }
        self.entries.insert(path, new_hash);
    }

    pub fn remove(&mut self, path: &PathBuf) {
        self.entries.remove(path);
        self.insertion_order.retain(|entry| entry != path);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.insertion_order.clear();
    }

    fn evict_oldest(&mut self) {
        while let Some(oldest_key) = self.insertion_order.pop_front() {
            if self.entries.remove(&oldest_key).is_some() {
                break;
            }
        }
    }
}

pub(super) fn content_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
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
}
