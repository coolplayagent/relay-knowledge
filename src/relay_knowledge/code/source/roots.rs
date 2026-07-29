pub(crate) const STRIPPABLE_SOURCE_ROOTS: &[&str] = &[
    "src/",
    "lib/",
    "Sources/",
    "external_deps/",
    "packages/",
    "modules/",
    "plugins/",
    "extensions/",
];

pub(crate) const NESTED_SOURCE_MARKERS: &[&str] = &[
    "/src/main/java/",
    "/src/test/java/",
    "/src/main/kotlin/",
    "/src/test/kotlin/",
    "/src/main/scala/",
    "/src/test/scala/",
    "/src/main/groovy/",
    "/src/test/groovy/",
];

const LEADING_SOURCE_MARKERS: &[&str] = &[
    "src/main/java/",
    "src/test/java/",
    "src/main/kotlin/",
    "src/test/kotlin/",
    "src/main/scala/",
    "src/test/scala/",
    "src/main/groovy/",
    "src/test/groovy/",
];

/// Returns repository-relative module identities for layouts that commonly
/// carry source outside a top-level src directory.
pub(crate) fn source_module_candidates(path: &str) -> Vec<String> {
    let path = normalize_layout_path(path);
    let mut candidates = Vec::new();
    push_candidate(&mut candidates, path.to_owned());
    for stripped in stripped_source_roots(path) {
        push_candidate(&mut candidates, stripped.to_owned());
    }

    candidates
}

pub(crate) fn source_relative_path(path: &str) -> String {
    source_module_candidates(path)
        .into_iter()
        .find(|candidate| candidate != path)
        .unwrap_or_else(|| normalize_layout_path(path).to_owned())
}

pub(crate) fn go_module_candidates(path: &str) -> Vec<String> {
    let path = normalize_layout_path(path);
    let mut candidates = source_module_candidates(path);
    if let Some(stripped) = path.strip_prefix("staging/src/") {
        push_candidate(&mut candidates, stripped.to_owned());
    }
    if let Some(stripped) = path.strip_prefix("vendor/") {
        push_candidate(&mut candidates, stripped.to_owned());
    }

    candidates
}

pub(crate) fn c_family_module_candidates(path: &str) -> Vec<String> {
    let mut candidates = source_module_candidates(path);
    for candidate in candidates.clone() {
        if let Some(include_path) = include_segment_path(&candidate) {
            push_candidate(&mut candidates, include_path.to_owned());
        }
        if let Some(stripped) = strip_include_segment(&candidate) {
            push_candidate(&mut candidates, stripped.to_owned());
        }
    }

    candidates
}

pub(crate) fn normalized_module_candidates(path: &str) -> Vec<String> {
    let path = normalize_layout_path(path).trim_start_matches("./");
    if path.is_empty() {
        Vec::new()
    } else {
        vec![path.to_owned()]
    }
}

fn stripped_source_roots(path: &str) -> Vec<&str> {
    let mut stripped = Vec::new();
    for marker in LEADING_SOURCE_MARKERS {
        if let Some(candidate) = path.strip_prefix(marker) {
            stripped.push(candidate);
        }
    }
    for root in STRIPPABLE_SOURCE_ROOTS {
        if let Some(candidate) = path.strip_prefix(root) {
            stripped.push(candidate);
        }
    }
    for marker in NESTED_SOURCE_MARKERS {
        if let Some((_, candidate)) = path.split_once(marker) {
            stripped.push(candidate);
        }
    }

    stripped
}

fn strip_include_segment(path: &str) -> Option<&str> {
    path.strip_prefix("include/")
        .or_else(|| path.split_once("/include/").map(|(_, suffix)| suffix))
}

fn include_segment_path(path: &str) -> Option<&str> {
    path.strip_prefix("include/").map(|_| path).or_else(|| {
        path.split_once("/include/").map(|(_, suffix)| {
            let include_start = path.len() - suffix.len() - "include/".len();
            &path[include_start..]
        })
    })
}

fn normalize_layout_path(path: &str) -> &str {
    let mut normalized = path.trim();
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped;
    }
    normalized.trim_end_matches('/')
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidate.is_empty() && !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

#[cfg(test)]
#[path = "roots_tests.rs"]
mod tests;
