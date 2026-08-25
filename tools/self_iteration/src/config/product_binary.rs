use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductBinaryProfile {
    Debug,
    Release,
}

impl ProductBinaryProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    pub fn binary_path(self, workspace: &Path) -> PathBuf {
        workspace
            .join("target")
            .join(self.as_str())
            .join("relay-knowledge")
    }

    pub fn for_evaluation_profile(profile: &str) -> Option<Self> {
        if profile == "smoke" {
            None
        } else {
            Some(Self::Release)
        }
    }

    pub fn legacy_for_evaluation_profile(profile: &str) -> Self {
        if profile == "fast" {
            Self::Debug
        } else {
            Self::Release
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "debug" => Some(Self::Debug),
            "release" => Some(Self::Release),
            _ => None,
        }
    }
}

pub(crate) fn run_matches_product_binary_profile(run: &Value, profile: &str) -> bool {
    let expected = ProductBinaryProfile::for_evaluation_profile(profile);
    match run.get("product_binary_profile") {
        Some(Value::Null) => expected.is_none(),
        Some(Value::String(value)) => ProductBinaryProfile::parse(value) == expected,
        Some(_) => false,
        None => Some(ProductBinaryProfile::legacy_for_evaluation_profile(profile)) == expected,
    }
}

#[cfg(test)]
#[path = "product_binary_tests.rs"]
mod tests;
