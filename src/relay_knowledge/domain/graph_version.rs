use serde::{Deserialize, Serialize};

/// Monotonic graph state version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphVersion(u64);

impl GraphVersion {
    /// The empty graph version used before storage is attached.
    pub const ZERO: Self = Self(0);

    /// Creates a graph version from its numeric representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric graph version.
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_numeric_graph_version() {
        assert_eq!(GraphVersion::ZERO.get(), 0);
        assert_eq!(GraphVersion::new(42).get(), 42);
    }
}
