pub(super) fn strip_comment(value: &str) -> &str {
    value
        .split("//")
        .next()
        .unwrap_or(value)
        .split('#')
        .next()
        .unwrap_or(value)
}

pub(super) fn replace_modules(rest: &str) -> Vec<&str> {
    rest.split("=>")
        .filter_map(|side| {
            side.split_whitespace()
                .next()
                .filter(|module| module_path(module))
        })
        .collect()
}

fn module_path(value: &str) -> bool {
    !value.starts_with('.') && !value.starts_with('/') && !value.contains('\\')
}
