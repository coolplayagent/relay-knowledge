//! Dependency-free stable identity hashing shared across repository layers.

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Incremental FNV-1a state for bounded identity construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StableHasher64 {
    state: u64,
}

impl StableHasher64 {
    pub(crate) const fn new() -> Self {
        Self {
            state: FNV_OFFSET_BASIS,
        }
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    pub(crate) const fn finish(self) -> u64 {
        self.state
    }
}

pub(crate) fn stable_hash64(bytes: &[u8]) -> u64 {
    let mut hasher = StableHasher64::new();
    hasher.update(bytes);
    hasher.finish()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
