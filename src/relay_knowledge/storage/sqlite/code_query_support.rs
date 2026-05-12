use crate::domain::CodeRetrievalLayer;

pub(super) fn score_text(query: &str, fields: impl IntoIterator<Item = impl AsRef<str>>) -> f64 {
    let haystack = fields
        .into_iter()
        .map(|field| field.as_ref().to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let mut score = 0.0;
    for token in query.split_whitespace() {
        if haystack.contains(token) {
            score += 1.0;
        }
    }

    score
}

pub(super) fn path_to_module_keys(path: &str) -> Vec<String> {
    let stem = path
        .trim_end_matches(".tsx")
        .trim_end_matches(".ts")
        .trim_end_matches(".rs")
        .trim_end_matches(".py");
    let slash_key = stem.replace(['/', '\\'], "::");
    let dotted_key = stem.replace(['/', '\\'], ".");
    let without_src = slash_key.strip_prefix("src::").unwrap_or(&slash_key);
    let mut keys = vec![slash_key.clone(), dotted_key, without_src.to_owned()];
    if !without_src.starts_with("crate") {
        keys.push(format!("crate::{without_src}"));
    }
    keys.sort();
    keys.dedup();

    keys
}

pub(super) fn path_matches_filter(path: &str, filter: &str) -> bool {
    let filter = filter.trim_end_matches(['/', '\\']);
    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

pub(super) fn language_id_for_path(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()? {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "ts" => Some("typescript"),
        "tsx" => Some("tsx"),
        _ => None,
    }
}

pub(super) fn module_import_matches(imported_module: &str, changed_module: &str) -> bool {
    imported_module
        .match_indices(changed_module)
        .any(|(start, value)| {
            let end = start + value.len();
            module_boundary(imported_module[..start].chars().next_back())
                && module_boundary(imported_module[end..].chars().next())
        })
}

fn module_boundary(character: Option<char>) -> bool {
    character
        .map(|value| matches!(value, ':' | '.' | '/' | '\\'))
        .unwrap_or(true)
}

pub(super) fn chunk_layers(parse_status: &str) -> Vec<CodeRetrievalLayer> {
    let mut layers = vec![CodeRetrievalLayer::Lexical];
    if parse_status != "parsed" {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}
