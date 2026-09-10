use serde::{Deserialize, Serialize};

/// A single Java compilation unit's namespace, collected from its original AST.
/// Incomplete evidence never proves the absence of a same-package type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaNamespaceEvidence {
    #[serde(default)]
    pub source_set: JavaSourceSet,
    pub package: String,
    pub top_level_types: Vec<String>,
    pub complete: bool,
}

impl JavaNamespaceEvidence {
    pub(crate) const MAX_PROJECTED_NAME_BYTES: usize = 65_536;

    /// Count persisted name bytes, including the package repeated in every type row.
    /// Storage adds its scope/path/row overhead to this lower-bound accounting.
    pub(crate) fn projected_name_bytes(&self) -> Option<usize> {
        let mut bytes = self
            .package
            .len()
            .checked_add(self.source_set.projected_bytes())?;
        if bytes > Self::MAX_PROJECTED_NAME_BYTES {
            return None;
        }
        if self.complete {
            for name in &self.top_level_types {
                bytes = bytes
                    .checked_add(self.package.len())?
                    .checked_add(self.source_set.projected_bytes())?
                    .checked_add(name.len())?;
                if bytes > Self::MAX_PROJECTED_NAME_BYTES {
                    return None;
                }
            }
        }
        Some(bytes)
    }
}

/// A configuration read whose short receiver relies on implicit java.lang lookup.
/// Its package is obtained from the file's namespace evidence, not duplicated per read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaImplicitPlatformRead {
    pub type_name: String,
}

#[cfg(test)]
#[path = "java_namespace_tests.rs"]
mod tests;

/// Static conventional Java source-set identity; missing legacy evidence is unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaSourceSet {
    Repository,
    Main {
        module_root: String,
    },
    Test {
        module_root: String,
    },
    #[default]
    Unknown,
}

impl JavaSourceSet {
    pub(crate) fn projected_bytes(&self) -> usize {
        match self {
            Self::Repository => "repository".len(),
            Self::Main { module_root } => module_root.len().saturating_add(4),
            Self::Test { module_root } => module_root.len().saturating_add(4),
            Self::Unknown => "unknown".len(),
        }
    }
}
