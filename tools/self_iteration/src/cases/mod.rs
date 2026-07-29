use std::{collections::BTreeMap, fs, path::Path};

use serde_json::{Map, Value};

include!("loading.rs");
include!("merge.rs");
include!("fields.rs");
include!("grouping.rs");

#[cfg(test)]
#[path = "merge_tests.rs"]
mod merge_tests;
