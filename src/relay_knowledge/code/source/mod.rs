//! Source discovery, Git access, filesystem snapshots, and source-layout rules.

pub(in crate::code) mod change_status;
pub(in crate::code) mod changes;
pub(in crate::code) mod declarations;
pub(in crate::code) mod filesystem;
pub(in crate::code) mod filters;
pub(in crate::code) mod git;
pub(in crate::code) mod gitlink;
pub(in crate::code) mod layout;
mod repository;
pub(in crate::code) mod resolution;
pub(crate) mod roots;

use crate::code::{CodeIndexError, ids, languages, parser, snapshot};
use filters as source_paths;
use gitlink as source_gitlink;
use layout as scope;
use repository as source;
use roots as source_roots;

pub(in crate::code) use repository::*;
