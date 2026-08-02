use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub(in crate::code) struct TrackedEntryScope {
    path_filters: Vec<String>,
    entry_filter: TrackedEntryFilter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TrackedEntryFilter {
    #[default]
    None,
    Empty,
    Nested,
    All,
}

impl TrackedEntryScope {
    #[cfg(test)]
    pub(in crate::code) fn all() -> Self {
        Self {
            path_filters: Vec::new(),
            entry_filter: TrackedEntryFilter::None,
        }
    }

    pub(in crate::code) fn empty() -> Self {
        Self {
            path_filters: Vec::new(),
            entry_filter: TrackedEntryFilter::Empty,
        }
    }

    pub(in crate::code) fn from_path_filters<'a>(
        filters: impl IntoIterator<Item = &'a String>,
    ) -> Self {
        Self {
            path_filters: normalized_filters(filters),
            entry_filter: TrackedEntryFilter::Nested,
        }
    }

    pub(in crate::code) fn from_entry_path_filters<'a>(
        filters: impl IntoIterator<Item = &'a String>,
    ) -> Self {
        Self {
            path_filters: normalized_filters(filters),
            entry_filter: TrackedEntryFilter::All,
        }
    }

    pub(super) fn excludes_all_entries(&self) -> bool {
        self.entry_filter == TrackedEntryFilter::Empty
    }

    pub(super) fn allows_submodule_expansion(&self, path: &str) -> bool {
        !self.excludes_all_entries()
            && (self.path_filters.is_empty()
                || self
                    .path_filters
                    .iter()
                    .any(|filter| path_overlaps_filter(path, filter)))
    }

    pub(super) fn allows_entry(&self, prefix: &str, path: &str) -> bool {
        let path = format!("{prefix}{path}");
        match self.entry_filter {
            TrackedEntryFilter::None => true,
            TrackedEntryFilter::Empty => false,
            TrackedEntryFilter::Nested if prefix.is_empty() => true,
            TrackedEntryFilter::Nested | TrackedEntryFilter::All => {
                self.path_filters.is_empty()
                    || self
                        .path_filters
                        .iter()
                        .any(|filter| path_matches_filter(&path, filter))
            }
        }
    }

    pub(super) fn entry_pathspecs(&self, prefix: &str) -> Option<EntryPathspecs> {
        if self.entry_filter == TrackedEntryFilter::All {
            return EntryPathspecs::from_filters(&self.path_filters);
        }
        if self.entry_filter != TrackedEntryFilter::Nested
            || prefix.is_empty()
            || self.path_filters.is_empty()
        {
            return None;
        }
        let prefix_path = prefix.trim_end_matches('/');
        let mut paths = Vec::new();
        for filter in &self.path_filters {
            if filter == "." || path_matches_filter(prefix_path, filter) {
                return None;
            }
            if let Some(child_filter) = filter.strip_prefix(prefix)
                && !child_filter.is_empty()
            {
                paths.push(child_filter.to_owned());
            }
        }

        EntryPathspecs::from_filters(&paths)
    }
}

pub(super) struct EntryPathspecs {
    pub(super) paths: Vec<String>,
    pub(super) gitlink_candidates: Vec<String>,
}

impl EntryPathspecs {
    fn from_filters(filters: &[String]) -> Option<Self> {
        let paths = filters
            .iter()
            .filter(|filter| !filter.is_empty())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut gitlink_candidates = BTreeSet::new();
        for filter in &paths {
            for (index, _) in filter.match_indices('/') {
                gitlink_candidates.insert(filter[..index].to_owned());
            }
        }
        gitlink_candidates.retain(|candidate| !paths.contains(candidate));
        (!paths.is_empty()).then(|| Self {
            paths: paths.into_iter().collect(),
            gitlink_candidates: gitlink_candidates.into_iter().collect(),
        })
    }
}

fn normalized_filters<'a>(filters: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    filters
        .into_iter()
        .map(|filter| normalize_path_filter(filter).to_owned())
        .filter(|filter| !filter.is_empty())
        .collect()
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

fn path_overlaps_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !path.is_empty()
        && !filter.is_empty()
        && (path == filter
            || path.starts_with(&format!("{filter}/"))
            || filter.starts_with(&format!("{path}/")))
}

fn path_matches_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }

    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
