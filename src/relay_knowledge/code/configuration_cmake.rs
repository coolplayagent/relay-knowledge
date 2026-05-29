use super::{unquote, valid_target};

pub(super) fn link_target(value: &str) -> bool {
    let value = unquote(value).trim();
    valid_target(value)
        && !link_keyword(value)
        && !library_file(value)
        && !value.starts_with('-')
        && !value.starts_with("$<")
        && !value.starts_with("LINKER:")
        && !value.starts_with("SHELL:")
}

pub(super) fn literal_target_name(value: &str) -> Option<&str> {
    let value = unquote(value).trim();
    valid_target(value).then_some(value)
}

pub(super) fn include_module(path: &str, module: &str) -> Option<String> {
    let module = unquote(module);
    if let Some(relative) = module.strip_prefix("${CMAKE_CURRENT_LIST_DIR}/") {
        return Some(join_parent(path, relative));
    }
    if module.contains('$') {
        None
    } else if module.ends_with(".cmake") {
        Some(module.to_owned())
    } else {
        Some(format!("{module}.cmake"))
    }
}

pub(super) fn subdirectory_module(path: &str, module: &str) -> Option<String> {
    let module = unquote(module).trim_end_matches('/');
    (!module.contains('$') && !absolute_path(module))
        .then(|| join_parent(path, &format!("{module}/CMakeLists.txt")))
}

pub(super) fn alias_target(args: &str) -> Option<&str> {
    let mut parts = args.split_whitespace();
    parts.next()?;
    if parts.next()?.eq_ignore_ascii_case("ALIAS") {
        parts.next().and_then(literal_target_name)
    } else {
        None
    }
}

fn join_parent(path: &str, relative: &str) -> String {
    path.rsplit_once('/').map_or_else(
        || relative.to_owned(),
        |(parent, _)| format!("{parent}/{relative}"),
    )
}

fn absolute_path(value: &str) -> bool {
    value.starts_with('/') || value.starts_with('\\') || windows_absolute_path(value)
}

fn windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn link_keyword(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "PRIVATE"
            | "PUBLIC"
            | "INTERFACE"
            | "LINK_PRIVATE"
            | "LINK_PUBLIC"
            | "LINK_INTERFACE_LIBRARIES"
            | "DEBUG"
            | "OPTIMIZED"
            | "GENERAL"
    )
}

fn library_file(value: &str) -> bool {
    value.starts_with('/')
        || value.contains('\\')
        || value.contains('/')
        || value.ends_with(".a")
        || value.ends_with(".lib")
        || value.ends_with(".dll")
        || value.ends_with(".dylib")
        || value.ends_with(".framework")
        || value.ends_with(".so")
        || value.contains(".so.")
}
