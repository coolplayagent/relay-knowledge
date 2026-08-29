use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use crate::identity::stable_hash64;

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

    pub(crate) fn snapshots(&self) -> Vec<(PathBuf, u64)> {
        self.insertion_order
            .iter()
            .filter_map(|path| self.entries.get(path).map(|hash| (path.clone(), *hash)))
            .collect()
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
    stable_hash64(bytes)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
