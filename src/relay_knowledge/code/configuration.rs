use configuration_calls::{cmake_calls, starlark_calls};
pub(super) use configuration_detection::{detect, manual_parse_status, recoverable_parse_error};
use configuration_properties::skip_continued_value_line;
use configuration_templates::{strip_go_comments, strip_jinja_comments};

#[path = "configuration_calls.rs"]
mod configuration_calls;
#[path = "configuration_cmake.rs"]
mod configuration_cmake;
#[path = "configuration_detection.rs"]
mod configuration_detection;
#[path = "configuration_dockerfile.rs"]
mod configuration_dockerfile;
#[path = "configuration_gomod.rs"]
mod configuration_gomod;
#[path = "configuration_gotemplate.rs"]
mod configuration_gotemplate;
#[path = "configuration_json.rs"]
mod configuration_json;
#[path = "configuration_make.rs"]
mod configuration_make;
#[path = "configuration_markdown.rs"]
mod configuration_markdown;
#[path = "configuration_ninja.rs"]
mod configuration_ninja;
#[path = "configuration_properties.rs"]
mod configuration_properties;
#[path = "configuration_starlark.rs"]
mod configuration_starlark;
#[path = "configuration_templates.rs"]
mod configuration_templates;
#[path = "configuration_xml.rs"]
mod configuration_xml;
#[path = "configuration_yaml.rs"]
mod configuration_yaml;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConfigFact {
    pub(super) name: String,
    pub(super) kind: &'static str,
    pub(super) range: ConfigRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConfigReference {
    pub(super) name: String,
    pub(super) kind: &'static str,
    pub(super) range: ConfigRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConfigImport {
    pub(super) module: String,
    pub(super) range: ConfigRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ConfigRange {
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) line_start: usize,
    pub(super) line_end: usize,
}

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
        "markdown" => markdown_facts(content, &mut definitions),
        "xml" => xml_facts(content, &mut definitions),
        "starlark" => starlark_facts(path, content, &mut definitions, &mut references),
        "make" => make_facts(content, &mut definitions, &mut references),
        "cmake" => cmake_facts(content, &mut definitions, &mut references),
        "dockerfile" => dockerfile_facts(content, &mut definitions, &mut references),
        "properties" | "toml" | "ini" | "yaml" | "json" => {
            key_value_facts(language_id, content, &mut definitions)
        }
        "gomod" => gomod_facts(content, &mut definitions, &mut references),
        "ninja" => ninja_facts(content, &mut definitions, &mut references),
        "jinja2" => jinja_facts(content, &mut definitions, &mut references),
        "gotemplate" => gotemplate_facts(path, content, &mut definitions, &mut references),
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
        "xml" => xml_imports(content, &mut imports),
        "cmake" => cmake_imports(path, content, &mut imports),
        "dockerfile" => dockerfile_imports(content, &mut imports),
        "starlark" => starlark_imports(content, &mut imports),
        "make" => make_imports(content, &mut imports),
        "jinja2" => jinja_imports(content, &mut imports),
        "gotemplate" => gotemplate_imports(content, &mut imports),
        "ninja" => ninja_imports(content, &mut imports),
        _ => {}
    }

    imports
}

fn markdown_facts(content: &str, definitions: &mut Vec<ConfigFact>) {
    for heading in configuration_markdown::headings(content) {
        push_definition(definitions, heading.name, "heading", heading.range);
    }
}

fn xml_facts(content: &str, definitions: &mut Vec<ConfigFact>) {
    let mut state = configuration_xml::ScanState::default();
    for line in source_lines(content) {
        for name in configuration_xml::element_names(line.text, &mut state) {
            push_definition(definitions, name, "element", line.range());
        }
    }
}

fn xml_imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut state = configuration_xml::ScanState::default();
    for line in source_lines(content) {
        for module in configuration_xml::import_modules(line.text, &mut state) {
            push_import(imports, module, line.range());
        }
    }
}

fn starlark_facts(
    path: &str,
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_load_call = false;
    let mut rule_call_balance = 0i32;
    for line in source_lines(content) {
        let trimmed = line.text.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let code = strip_inline_hash_comment(trimmed).trim_end();
        if in_load_call {
            in_load_call = !code.contains(')');
            continue;
        }
        if call_args_prefix(code, "load").is_some() {
            in_load_call = !code.contains(')');
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("def ").and_then(call_name) {
            push_definition(definitions, name, "function", line.range());
        }
        let starts_rule_call = starlark_call_start(code).is_some();
        if (rule_call_balance > 0 || starts_rule_call)
            && let Some(name) = starlark_name_argument(code)
        {
            push_definition(definitions, name, "target", line.range());
            if let Some(qualified) = configuration_starlark::qualified_target_name(path, name) {
                push_definition(definitions, qualified, "target", line.range());
            }
        }
        if rule_call_balance > 0 || starts_rule_call {
            rule_call_balance = (rule_call_balance + paren_delta(code)).max(0);
        }
        for label in quoted_labels(code)
            .into_iter()
            .filter_map(|label| configuration_starlark::target_label(path, label))
        {
            push_reference(references, label, "target", line.range());
        }
    }
}

fn starlark_imports(content: &str, imports: &mut Vec<ConfigImport>) {
    for call in starlark_calls(content, "load") {
        let text = configuration_starlark::load_text_without_comments(&call.text);
        if let Some(value) = first_quoted(&text) {
            push_import(imports, value, call.range);
        }
    }
}

fn make_facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_define = false;
    for line in configuration_make::logical_lines(content) {
        if line.text.starts_with('\t') {
            continue;
        }
        let trimmed = line.text.trim();
        if configuration_make::skip_define_body(trimmed, &mut in_define) {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = assignment(configuration_make::assignment_line(trimmed)) {
            push_definition(definitions, name, "variable", line.range);
            for reference in configuration_make::variables(strip_line_comment(value)) {
                push_reference(references, reference, "variable", line.range);
            }
            continue;
        }
        if let Some(rule) = configuration_make::rule_parts(trimmed) {
            let deps = strip_line_comment(rule.deps).trim();
            if assignment(deps).is_some() {
                continue;
            }
            let mut has_target = false;
            for target in rule
                .targets
                .split_whitespace()
                .filter(|target| configuration_make::valid_target(target))
            {
                has_target = true;
                push_definition(definitions, target, "target", line.range);
            }
            if has_target {
                for dep in deps.split_whitespace().filter(|dep| {
                    configuration_make::valid_reference_target(dep, rule.static_pattern)
                }) {
                    push_reference(references, dep, "target", line.range);
                }
                for variable in configuration_make::variables(deps) {
                    push_reference(references, variable, "variable", line.range);
                }
            }
        }
    }
}

fn make_imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut in_define = false;
    for line in configuration_make::logical_lines(content) {
        if line.text.starts_with('\t') {
            continue;
        }
        let trimmed = line.text.trim();
        if configuration_make::skip_define_body(trimmed, &mut in_define) {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = configuration_make::include_args(trimmed) else {
            continue;
        };
        for module in strip_line_comment(rest)
            .split_whitespace()
            .filter_map(configuration_make::static_include_module)
        {
            push_import(imports, module, line.range);
        }
    }
}

fn cmake_facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    for call in cmake_calls(content) {
        for command in ["add_library", "add_executable", "add_custom_target"] {
            if call.command.eq_ignore_ascii_case(command)
                && let Some(name) = call.args.split_whitespace().next()
                && let Some(name) = configuration_cmake::literal_target_name(name)
            {
                push_definition(definitions, name, "target", call.range);
            }
        }
        if matches!(call.command.as_str(), "add_library" | "add_executable")
            && let Some(target) = configuration_cmake::alias_target(&call.args)
        {
            push_reference(references, target, "target", call.range);
        }
        if call.command.eq_ignore_ascii_case("target_link_libraries") {
            for target in call
                .args
                .split_whitespace()
                .skip(1)
                .filter(|part| configuration_cmake::link_target(part))
            {
                push_reference(references, target, "target", call.range);
            }
        }
        if call.command.eq_ignore_ascii_case("add_dependencies") {
            for target in call
                .args
                .split_whitespace()
                .skip(1)
                .filter_map(configuration_cmake::literal_target_name)
            {
                push_reference(references, target, "target", call.range);
            }
        }
        if call.command.eq_ignore_ascii_case("set")
            && let Some(name) = call.args.split_whitespace().next()
            && valid_config_key(name)
        {
            push_definition(definitions, name, "variable", call.range);
        }
    }
    for line in source_lines(content) {
        let trimmed = line.text.trim_start();
        for reference in cmake_variables(strip_line_comment(trimmed)) {
            push_reference(references, reference, "variable", line.range());
        }
    }
}

fn cmake_imports(path: &str, content: &str, imports: &mut Vec<ConfigImport>) {
    for call in cmake_calls(content) {
        if call.command.eq_ignore_ascii_case("include")
            && let Some(module) = call.args.split_whitespace().next()
            && let Some(module) = configuration_cmake::include_module(path, module)
        {
            push_import(imports, module, call.range);
        }
        if call.command.eq_ignore_ascii_case("add_subdirectory")
            && let Some(module) = call.args.split_whitespace().next()
            && let Some(module) = configuration_cmake::subdirectory_module(path, module)
        {
            push_import(imports, module, call.range);
        }
    }
}

fn dockerfile_facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut stages = Vec::<String>::new();
    for line in configuration_dockerfile::logical_lines(content) {
        let words = line.text.split_whitespace().collect::<Vec<_>>();
        if words
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case("FROM"))
        {
            if let Some(stage) = configuration_dockerfile::stage_name(&words) {
                push_definition(definitions, stage, "stage", line.range);
                stages.push(stage.to_owned());
            }
        }
        for source in configuration_dockerfile::copy_from_sources(&words) {
            if stages.iter().any(|stage| stage == source) {
                push_reference(references, source, "stage", line.range);
            }
        }
    }
}

fn dockerfile_imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut stages = Vec::<String>::new();
    for line in configuration_dockerfile::logical_lines(content) {
        let words = line.text.split_whitespace().collect::<Vec<_>>();
        if words
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case("FROM"))
        {
            if let Some(image) = configuration_dockerfile::from_image(&words)
                && !stages.iter().any(|stage| stage == image)
            {
                push_import(imports, image, line.range);
            }
            if let Some(stage) = configuration_dockerfile::stage_name(&words) {
                stages.push(stage.to_owned());
            }
        }
        for source in configuration_dockerfile::copy_from_sources(&words) {
            if !stages.iter().any(|stage| stage == source) {
                push_import(imports, source, line.range);
            }
        }
    }
}

fn key_value_facts(language_id: &str, content: &str, definitions: &mut Vec<ConfigFact>) {
    let mut yaml_block = configuration_yaml::BlockScalarTracker::default();
    let mut properties_continuation = false;
    for line in source_lines(content) {
        let trimmed = line.text.trim();
        if language_id == "yaml" && yaml_block.should_skip(line.text, trimmed) {
            continue;
        }
        if language_id == "properties"
            && skip_continued_value_line(line.text, trimmed, &mut properties_continuation)
        {
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('!')
            || trimmed.starts_with("//")
        {
            continue;
        }
        if language_id == "json" {
            for key in configuration_json::object_keys(trimmed)
                .into_iter()
                .filter(|key| valid_config_key(key))
            {
                push_definition(definitions, key, "config_key", line.range());
            }
            continue;
        }
        if language_id == "toml"
            && let Some(section) = toml_section_name(trimmed)
        {
            push_definition(definitions, section, "section", line.range());
            continue;
        }
        if language_id == "ini" && trimmed.starts_with('[') && trimmed.ends_with(']') {
            push_definition(
                definitions,
                &trimmed[1..trimmed.len() - 1],
                "section",
                line.range(),
            );
            continue;
        }
        let key = if language_id == "yaml" {
            configuration_yaml::mapping_key(trimmed).map(config_key_prefix)
        } else {
            trimmed
                .split_once('=')
                .or_else(|| trimmed.split_once(':'))
                .map(|(key, _)| config_key_prefix(key))
                .or_else(|| properties_space_key(language_id, trimmed))
        };
        if let Some(key) = key.filter(|key| valid_config_key(key)) {
            push_definition(definitions, key, "config_key", line.range());
        }
    }
}

fn gomod_facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_require_block = false;
    let mut in_replace_block = false;
    for line in source_lines(content) {
        let trimmed = line.text.trim();
        let code = configuration_gomod::strip_comment(trimmed).trim();
        if code.starts_with("require (") {
            in_require_block = true;
            continue;
        }
        if code.starts_with("replace (") {
            in_replace_block = true;
            continue;
        }
        if in_require_block && code == ")" {
            in_require_block = false;
            continue;
        }
        if in_replace_block && code == ")" {
            in_replace_block = false;
            continue;
        }
        if let Some(module) = code.strip_prefix("module ").map(str::trim) {
            push_definition(definitions, module, "module", line.range());
        }
        if let Some(module) = code
            .strip_prefix("require ")
            .and_then(|value| value.split_whitespace().next())
        {
            push_reference(references, module, "dependency", line.range());
        }
        if in_require_block
            && !code.is_empty()
            && let Some(module) = code.split_whitespace().next()
        {
            push_reference(references, module, "dependency", line.range());
        }
        if in_replace_block && !code.is_empty() {
            for module in configuration_gomod::replace_modules(code) {
                push_reference(references, module, "dependency", line.range());
            }
        }
        if let Some(rest) = code.strip_prefix("replace ") {
            for module in configuration_gomod::replace_modules(rest) {
                push_reference(references, module, "dependency", line.range());
            }
        }
    }
}

fn ninja_facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    for line in configuration_ninja::logical_lines(content) {
        let trimmed = line.text.trim();
        if let Some(rule) = trimmed.strip_prefix("rule ").map(str::trim) {
            push_definition(definitions, rule, "rule", line.range);
        }
        if let Some(rest) = trimmed.strip_prefix("build ")
            && let Some((outputs, inputs)) = rest.split_once(':')
        {
            for output in outputs.split_whitespace().filter(|part| valid_target(part)) {
                push_definition(definitions, output, "target", line.range);
            }
            for input in inputs
                .split_whitespace()
                .skip(1)
                .filter(|part| valid_target(part))
            {
                push_reference(references, input, "target", line.range);
            }
        }
        if let Some((name, value)) = assignment(trimmed) {
            push_definition(definitions, name, "variable", line.range);
            for reference in configuration_ninja::variables(value) {
                push_reference(references, reference, "variable", line.range);
            }
        }
    }
}

fn ninja_imports(content: &str, imports: &mut Vec<ConfigImport>) {
    for line in source_lines(content) {
        let trimmed = strip_line_comment(line.text).trim();
        for prefix in ["include ", "subninja "] {
            if let Some(module) = trimmed
                .strip_prefix(prefix)
                .map(str::trim)
                .and_then(configuration_ninja::static_include_module)
            {
                push_import(imports, module, line.range());
            }
        }
    }
}

fn jinja_facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_jinja_comments(line.text, &mut in_comment);
        for action in ["block", "macro"] {
            for name in jinja_actions(&text, action)
                .into_iter()
                .filter_map(identifier_prefix)
            {
                push_definition(definitions, name, "template", line.range());
            }
        }
        for name in template_variables(&text) {
            push_reference(references, name, "variable", line.range());
        }
    }
}

fn jinja_imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_jinja_comments(line.text, &mut in_comment);
        for action in ["include", "extends", "import", "from"] {
            for body in jinja_actions(&text, action) {
                let values = if action == "include" {
                    configuration_templates::quoted_values(body)
                } else {
                    first_quoted(body).into_iter().collect()
                };
                for value in values {
                    push_import(imports, value, line.range());
                }
            }
        }
    }
}

fn gotemplate_facts(
    path: &str,
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    if let Some(language_id) = configuration_detection::static_template_language(path) {
        key_value_facts(language_id, content, definitions);
    }
    if !configuration_gotemplate::syntax(content) {
        return;
    }
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_go_comments(line.text, &mut in_comment);
        for action in ["define", "block"] {
            for name in configuration_gotemplate::actions(&text, action)
                .into_iter()
                .filter_map(first_quoted)
            {
                push_definition(definitions, name, "template", line.range());
            }
        }
        for action in ["template", "include", "block"] {
            for name in configuration_gotemplate::actions(&text, action)
                .into_iter()
                .filter_map(first_quoted)
            {
                push_reference(references, name, "template", line.range());
            }
        }
    }
}

fn gotemplate_imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_go_comments(line.text, &mut in_comment);
        for action in ["template", "include", "block"] {
            for name in configuration_gotemplate::actions(&text, action)
                .into_iter()
                .filter_map(first_quoted)
            {
                push_import(imports, name, line.range());
            }
        }
    }
}

fn jinja_actions<'a>(line: &'a str, action: &str) -> Vec<&'a str> {
    let mut actions = Vec::new();
    for part in line.split("{%").skip(1) {
        let part = part
            .split("%}")
            .next()
            .unwrap_or(part)
            .trim_start_matches('-')
            .trim_start();
        if let Some(rest) = part.strip_prefix(action) {
            if rest
                .chars()
                .next()
                .is_none_or(|character| !valid_config_character(character))
            {
                actions.push(rest.trim_start());
            }
        }
    }

    actions
}

fn source_lines(content: &str) -> Vec<ConfigLine<'_>> {
    let mut lines = Vec::new();
    let mut byte_start = 0usize;
    for (index, raw_line) in content.split_inclusive('\n').enumerate() {
        let without_lf = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let text = without_lf.strip_suffix('\r').unwrap_or(without_lf);
        lines.push(ConfigLine {
            number: index + 1,
            byte_start,
            byte_end: byte_start + text.len(),
            text,
        });
        byte_start += raw_line.len();
    }
    if content.is_empty() {
        lines.push(ConfigLine {
            number: 1,
            byte_start: 0,
            byte_end: 0,
            text: "",
        });
    }

    lines
}

struct ConfigLine<'a> {
    number: usize,
    byte_start: usize,
    byte_end: usize,
    text: &'a str,
}

impl ConfigLine<'_> {
    fn range(&self) -> ConfigRange {
        ConfigRange {
            byte_start: self.byte_start,
            byte_end: self.byte_end,
            line_start: self.number,
            line_end: self.number,
        }
    }
}

fn push_definition(
    definitions: &mut Vec<ConfigFact>,
    name: impl AsRef<str>,
    kind: &'static str,
    range: ConfigRange,
) {
    let name = clean_name(name.as_ref());
    if !name.is_empty()
        && !definitions.iter().any(|existing| {
            existing.name == name && existing.kind == kind && existing.range == range
        })
    {
        definitions.push(ConfigFact { name, kind, range });
    }
}

fn push_reference(
    references: &mut Vec<ConfigReference>,
    name: impl AsRef<str>,
    kind: &'static str,
    range: ConfigRange,
) {
    let name = clean_name(name.as_ref());
    if !name.is_empty()
        && !references.iter().any(|existing| {
            existing.name == name && existing.kind == kind && existing.range == range
        })
    {
        references.push(ConfigReference { name, kind, range });
    }
}

fn push_import(imports: &mut Vec<ConfigImport>, module: impl AsRef<str>, range: ConfigRange) {
    let module = clean_name(module.as_ref());
    if !module.is_empty()
        && !imports
            .iter()
            .any(|existing| existing.module == module && existing.range == range)
    {
        imports.push(ConfigImport { module, range });
    }
}

fn clean_name(value: &str) -> String {
    unquote(value)
        .trim()
        .trim_end_matches(',')
        .trim_end_matches(')')
        .trim_end_matches('}')
        .to_owned()
}

fn unquote(value: &str) -> &str {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
}

fn config_key_prefix(value: &str) -> &str {
    value
        .trim()
        .trim_start_matches('-')
        .trim()
        .trim_start_matches('{')
        .trim_start_matches('[')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
}

fn toml_section_name(value: &str) -> Option<&str> {
    value
        .strip_prefix("[[")
        .and_then(|section| section.strip_suffix("]]"))
        .or_else(|| {
            value
                .strip_prefix('[')
                .and_then(|section| section.strip_suffix(']'))
        })
        .map(str::trim)
        .filter(|section| valid_config_key(section))
}

fn properties_space_key<'a>(language_id: &str, value: &'a str) -> Option<&'a str> {
    (language_id == "properties")
        .then(|| value.split_whitespace().next())
        .flatten()
}

fn first_quoted(value: &str) -> Option<&str> {
    let quote = value
        .chars()
        .find(|character| matches!(character, '"' | '\''))?;
    let start = value.find(quote)? + quote.len_utf8();
    let rest = &value[start..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
}

fn quoted_labels(line: &str) -> Vec<&str> {
    line.split(['"', '\''])
        .filter(|part| part.starts_with("//") || part.starts_with(':') || part.starts_with('@'))
        .collect()
}

fn starlark_name_argument(line: &str) -> Option<&str> {
    let mut offset = 0usize;
    while let Some(index) = line[offset..].find("name") {
        let start = offset + index;
        let before = line[..start].chars().next_back();
        let rest = &line[start + "name".len()..];
        if before.is_none_or(|character| !valid_config_character(character)) {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix('=').and_then(first_quoted) {
                return Some(value);
            }
        }
        offset = start + "name".len();
    }

    None
}

fn call_name(value: &str) -> Option<&str> {
    value
        .split('(')
        .next()
        .map(str::trim)
        .filter(|name| valid_config_key(name))
}

fn call_args_prefix<'a>(line: &'a str, command: &str) -> Option<&'a str> {
    if !line
        .get(..command.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(command))
    {
        return None;
    }
    let rest = line.get(command.len()..)?.trim_start();
    rest.strip_prefix('(')
}

fn starlark_call_start(line: &str) -> Option<&str> {
    let (command, _) = line.split_once('(')?;
    let command = command.trim();
    valid_config_key(command).then_some(command)
}

fn paren_delta(value: &str) -> i32 {
    value.chars().fold(0, |balance, character| {
        balance
            + match character {
                '(' => 1,
                ')' => -1,
                _ => 0,
            }
    })
}

fn assignment(line: &str) -> Option<(&str, &str)> {
    for marker in ["?=", "+=", ":=", "="] {
        if let Some((left, right)) = line.split_once(marker) {
            let name = left.trim();
            if valid_config_key(name) {
                return Some((name, right.trim()));
            }
        }
    }

    None
}

fn cmake_variables(line: &str) -> Vec<&str> {
    variable_references(line, "${", "}")
}

fn variable_references<'a>(line: &'a str, prefix: &str, suffix: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    for part in line.split(prefix).skip(1) {
        let value = if suffix == " " {
            part.split(|character: char| !valid_config_character(character))
                .next()
        } else {
            part.split(suffix).next()
        };
        if let Some(value) = value.filter(|value| valid_config_key(value)) {
            values.push(value);
        }
    }

    values
}

fn template_variables(line: &str) -> Vec<&str> {
    line.split("{{")
        .skip(1)
        .filter_map(|part| part.split("}}").next())
        .map(str::trim)
        .filter_map(|part| part.split(['|', '.', ' ', '(']).next())
        .map(str::trim)
        .filter(|name| valid_config_key(name))
        .collect()
}

fn identifier_prefix(value: &str) -> Option<&str> {
    value
        .split(|character: char| !valid_config_character(character))
        .next()
        .filter(|name| valid_config_key(name))
}

fn valid_target(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with('.')
        && value
            .chars()
            .all(|character| valid_config_character(character) || matches!(character, '/' | ':'))
}

fn valid_config_key(value: &str) -> bool {
    !value.is_empty() && value.chars().all(valid_config_character)
}

fn valid_config_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/' | ':')
}

fn strip_line_comment(value: &str) -> &str {
    value.split('#').next().unwrap_or(value)
}

fn strip_inline_hash_comment(value: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if character == '#' && quote.is_none() {
            return &value[..index];
        }
    }

    value
}
