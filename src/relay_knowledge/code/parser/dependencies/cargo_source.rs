//! Classifies Cargo lockfile sources as external or workspace-local.

pub(super) fn cargo_lock_source_is_external(source: Option<&str>) -> bool {
    source.is_some_and(|source| {
        source.starts_with("registry+")
            || source.starts_with("git+")
            || source.starts_with("sparse+")
    })
}
