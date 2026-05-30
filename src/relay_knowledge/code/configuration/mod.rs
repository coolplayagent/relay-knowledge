mod calls;
mod detection;
mod key_values;
mod languages;
mod model;
mod source;

pub(super) use detection::{detect, manual_parse_status, recoverable_parse_error};
pub(super) use model::{ConfigFact, ConfigImport, ConfigRange, ConfigReference, ConfigValueKind};

pub(super) fn doc_comment_text<'a>(trimmed: &'a str, language_id: &str) -> Option<&'a str> {
    match language_id {
        "markdown" | "json" | "gomod" => None,
        "xml" => trimmed
            .strip_prefix("<!--")
            .and_then(|value| value.strip_suffix("-->"))
            .map(str::trim),
        "cmake" | "dockerfile" | "make" | "ninja" | "properties" | "toml" | "ini" | "yaml"
        | "starlark" => trimmed.strip_prefix('#').map(str::trim),
        "jinja2" | "gotemplate" => trimmed
            .strip_prefix("{#")
            .and_then(|value| value.strip_suffix("#}"))
            .map(str::trim),
        _ => None,
    }
}

pub(super) fn structured_facts(
    path: &str,
    language_id: &str,
    content: &str,
) -> (Vec<ConfigFact>, Vec<ConfigReference>) {
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    match language_id {
        "xml" => languages::xml::facts(content, &mut definitions),
        "starlark" => languages::starlark::facts(path, content, &mut definitions, &mut references),
        "make" => languages::make::facts(content, &mut definitions, &mut references),
        "cmake" => languages::cmake::facts(content, &mut definitions, &mut references),
        "dockerfile" => languages::dockerfile::facts(content, &mut definitions, &mut references),
        "properties" | "toml" | "ini" | "yaml" | "json" => {
            key_values::facts(language_id, content, &mut definitions)
        }
        "gomod" => languages::go_mod::facts(content, &mut definitions, &mut references),
        "ninja" => languages::ninja::facts(content, &mut definitions, &mut references),
        "jinja2" => languages::jinja::facts(content, &mut definitions, &mut references),
        "gotemplate" => {
            languages::go_template::facts(path, content, &mut definitions, &mut references)
        }
        _ => {}
    }

    for definition in &mut definitions {
        if definition.name.is_empty() {
            definition.name = path.to_owned();
        }
    }

    (definitions, references)
}

pub(super) fn structured_imports(
    path: &str,
    language_id: &str,
    content: &str,
) -> Vec<ConfigImport> {
    let mut imports = Vec::new();
    match language_id {
        "xml" => languages::xml::imports(content, &mut imports),
        "cmake" => languages::cmake::imports(path, content, &mut imports),
        "dockerfile" => languages::dockerfile::imports(content, &mut imports),
        "starlark" => languages::starlark::imports(content, &mut imports),
        "make" => languages::make::imports(content, &mut imports),
        "jinja2" => languages::jinja::imports(content, &mut imports),
        "gotemplate" => languages::go_template::imports(content, &mut imports),
        "ninja" => languages::ninja::imports(content, &mut imports),
        _ => {}
    }

    imports
}
