use super::{
    languages::language_id,
    parser::{dependency_manifest_language_ids, dependency_manifest_overrides_default_exclusion},
    source::source_language_filter_allows,
    source_paths::{
        DEFAULT_EXCLUDED_EXTENSIONS, DEFAULT_EXCLUDED_FILENAMES, FILESYSTEM_AUTO_DISCOVERY_FILTERS,
        FILESYSTEM_BROAD_SEGMENTS, FILESYSTEM_DEFAULT_SOURCE_ROOTS,
    },
    source_roots::STRIPPABLE_SOURCE_ROOTS,
};

#[derive(Debug, Clone, Default)]
pub(in crate::code) struct FileSystemScanPolicy {
    pub(in crate::code) explicit_root_scope: bool,
    pub(in crate::code) path_scope_denied: bool,
    pub(in crate::code) broad_directory_filters: Vec<String>,
    pub(in crate::code) path_scope_filters: Vec<String>,
    pub(in crate::code) language_filter_sets: Vec<Vec<String>>,
}

impl FileSystemScanPolicy {
    #[cfg(test)]
    pub(in crate::code) fn from_path_filters<'a>(
        path_filters: impl IntoIterator<Item = &'a String>,
    ) -> Self {
        Self::from_path_and_language_filters(path_filters, &[], &[])
    }

    pub(in crate::code) fn from_path_and_language_filters<'a>(
        path_filters: impl IntoIterator<Item = &'a String>,
        registration_language_filters: &[String],
        selector_language_filters: &[String],
    ) -> Self {
        let normalized = path_filters
            .into_iter()
            .map(|filter| normalize_path_filter(filter))
            .filter(|filter| !filter.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let language_filter_sets = [registration_language_filters, selector_language_filters]
            .into_iter()
            .filter(|filters| !filters.is_empty())
            .map(<[String]>::to_vec)
            .collect();

        Self {
            explicit_root_scope: normalized.iter().any(|filter| filter == "."),
            path_scope_denied: false,
            broad_directory_filters: normalized
                .iter()
                .filter(|filter| filter.as_str() != ".")
                .cloned()
                .collect(),
            path_scope_filters: normalized
                .into_iter()
                .filter(|filter| filter != ".")
                .collect(),
            language_filter_sets,
        }
    }

    pub(in crate::code) fn with_denied_path_scope(mut self) -> Self {
        self.path_scope_denied = true;
        self
    }

    pub(in crate::code) fn includes_broad_directory(&self, directory: &str) -> bool {
        if self.explicit_root_scope {
            return true;
        }

        self.broad_directory_filters
            .iter()
            .any(|filter| filter == directory || filter.starts_with(&format!("{directory}/")))
    }

    pub(in crate::code) fn hash_includes_path(&self, path: &str) -> bool {
        if self.explicit_root_scope {
            return true;
        }
        if self.path_scope_filters.is_empty() {
            return filesystem_default_source_allows(path);
        }

        self.path_scope_filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
    }

    pub(in crate::code) fn file_preset_allows_hash(&self, path: &str) -> bool {
        !source_default_file_preset_excludes(path)
            || dependency_manifest_overrides_default_exclusion(path)
            || explicit_path_filter_opts_into_default_file_exclusion(
                path,
                self.path_scope_filters.iter(),
            )
    }

    pub(in crate::code) fn language_allows_hash(&self, path: &str) -> bool {
        self.language_filter_sets
            .iter()
            .all(|filters| source_language_filter_allows(path, filters))
    }

    pub(in crate::code) fn should_descend_directory(&self, directory: &str) -> bool {
        if self.explicit_root_scope {
            return true;
        }
        if self.path_scope_filters.is_empty() {
            return filesystem_default_directory_can_contribute(directory);
        }

        if self
            .path_scope_filters
            .iter()
            .any(|filter| path_overlaps_filter(directory, filter))
        {
            return true;
        }

        self.path_scope_filters
            .iter()
            .any(|filter| filesystem_filter_can_discover_roots(filter))
            && discoverable_source_directory(directory)
    }

    pub(in crate::code) fn path_scope_filters(&self) -> &[String] {
        if self.explicit_root_scope {
            return &[];
        }

        &self.path_scope_filters
    }
}
pub(in crate::code) fn filesystem_default_source_allows(path: &str) -> bool {
    let normalized = normalize_path_filter(path);
    if normalized
        .split('/')
        .any(|segment| FILESYSTEM_BROAD_SEGMENTS.contains(&segment))
    {
        return false;
    }
    if !source_path_has_indexable_content(normalized) {
        return false;
    }
    let mut segments = normalized.split('/');
    let Some(first) = segments.next() else {
        return false;
    };
    if segments.next().is_none() {
        return true;
    }

    FILESYSTEM_DEFAULT_SOURCE_ROOTS.contains(&first)
}

fn filesystem_default_directory_can_contribute(directory: &str) -> bool {
    let directory = normalize_path_filter(directory);
    let Some(first) = directory.split('/').next() else {
        return false;
    };

    FILESYSTEM_DEFAULT_SOURCE_ROOTS.contains(&first)
}

pub(in crate::code) fn source_path_has_indexable_content(path: &str) -> bool {
    language_id(path).is_some() || dependency_manifest_language_ids(path).is_some()
}

pub(in crate::code) fn source_default_file_preset_excludes(path: &str) -> bool {
    let normalized = normalize_path_filter(path);
    if normalized
        .rsplit('/')
        .next()
        .is_some_and(|file_name| DEFAULT_EXCLUDED_FILENAMES.contains(&file_name))
    {
        return true;
    }
    normalized
        .rsplit_once('.')
        .map(|(_, extension)| {
            DEFAULT_EXCLUDED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
        .unwrap_or(false)
}

pub(in crate::code) fn explicit_path_filter_opts_into_default_file_exclusion<'a>(
    path: &str,
    filters: impl IntoIterator<Item = &'a String>,
) -> bool {
    let path_extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    filters.into_iter().any(|filter| {
        let filter = normalize_path_filter(filter);
        if filter.is_empty() || filter == "." {
            return false;
        }
        let filter_segments = filter.split('/').collect::<Vec<_>>();
        let targets_default_exclusion = filter_segments.iter().any(|segment| {
            DEFAULT_EXCLUDED_FILENAMES.contains(segment)
                || segment
                    .rsplit_once('.')
                    .map(|(_, ext)| ext.to_ascii_lowercase())
                    .is_some_and(|extension| {
                        DEFAULT_EXCLUDED_EXTENSIONS.contains(&extension.as_str())
                    })
        });
        if !targets_default_exclusion {
            return false;
        }
        path_matches_filter(path, filter)
            || filter.strip_prefix("*.").is_some_and(|extension| {
                path_extension.as_deref() == Some(&extension.to_ascii_lowercase())
            })
    })
}

pub(in crate::code) fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim().trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

fn path_matches_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
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

fn filesystem_filter_can_discover_roots(filter: &str) -> bool {
    let filter = normalize_path_filter(filter);
    FILESYSTEM_AUTO_DISCOVERY_FILTERS.contains(&filter)
}

fn discoverable_source_directory(directory: &str) -> bool {
    let directory = normalize_path_filter(directory);
    let Some(first_segment) = directory.split('/').next() else {
        return false;
    };
    if FILESYSTEM_AUTO_DISCOVERY_FILTERS.contains(&first_segment) {
        return true;
    }
    STRIPPABLE_SOURCE_ROOTS
        .iter()
        .map(|root| root.trim_end_matches('/'))
        .any(|root| root == first_segment)
}
