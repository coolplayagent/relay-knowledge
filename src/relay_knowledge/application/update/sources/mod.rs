mod crates_io;
mod github;
mod metadata;

pub(super) use crates_io::fetch_crates_release;
pub(super) use github::fetch_github_release;
