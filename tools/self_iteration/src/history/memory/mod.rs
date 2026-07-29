use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{git_ops::changed_paths_from_diff, history};

include!("api.rs");
include!("records.rs");
include!("store.rs");
include!("summaries.rs");
include!("metadata.rs");

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
