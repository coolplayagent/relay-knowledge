use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::domain::{CodeImportRecord, RepositoryCodeRange};

use super::{
    super::{CodeIndexError, SnapshotBuild, stable_id},
    nodes::{SyntaxRange, compact_whitespace, node_text, push_children_reverse, syntax_range},
};

pub(super) fn collect_imports(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    root: Node<'_>,
) -> Result<Vec<CodeImportRecord>, CodeIndexError> {
    let mut imports = ImportCollector::new(build, path, file_id);
    let mut stack = Vec::with_capacity(root.child_count().saturating_add(1));
    stack.push(root);
    while let Some(node) = stack.pop() {
        if let Some((module, range)) = javascript_like_dynamic_import(language_id, content, node) {
            imports.push_record(module, &range)?;
        } else if let Some((module, range)) = javascript_like_re_export(language_id, content, node)
        {
            imports.push_record(module, &range)?;
        } else if is_import_node(language_id, node) {
            imports.push_records(language_id, content, node)?;
        }
        push_children_reverse(node, &mut stack);
    }
    imports.push_script_line_imports(language_id, content)?;

    Ok(imports.into_records())
}

struct ImportCollector<'a> {
    build: &'a SnapshotBuild,
    path: &'a str,
    file_id: &'a str,
    seen_import_ids: BTreeSet<String>,
    records: Vec<CodeImportRecord>,
}

impl<'a> ImportCollector<'a> {
    fn new(build: &'a SnapshotBuild, path: &'a str, file_id: &'a str) -> Self {
        Self {
            build,
            path,
            file_id,
            seen_import_ids: BTreeSet::new(),
            records: Vec::new(),
        }
    }

    fn into_records(self) -> Vec<CodeImportRecord> {
        self.records
    }

    fn push_records(
        &mut self,
        language_id: &str,
        content: &str,
        node: Node<'_>,
    ) -> Result<(), CodeIndexError> {
        let module_text = node_text(content, node);
        let modules = if language_id == "go" {
            go_import_specs(&module_text)
        } else {
            Vec::new()
        };
        let modules = if modules.is_empty() {
            vec![compact_whitespace(&module_text)]
        } else {
            modules
        };
        let range = syntax_range(node);
        for module in modules {
            self.push_record(module, &range)?;
        }

        Ok(())
    }

    fn push_record(&mut self, module: String, range: &SyntaxRange) -> Result<(), CodeIndexError> {
        let module = module.trim().to_owned();
        if module.is_empty() || module == "import" {
            return Ok(());
        }
        let byte_start = range.byte_start.to_string();
        let byte_end = range.byte_end.to_string();
        let line_start = range.line_start.to_string();
        let line_end = range.line_end.to_string();
        let import_id = stable_id(
            "import",
            [
                &self.build.repository_id,
                &self.build.source_scope,
                self.path,
                &module,
                &byte_start,
                &byte_end,
                &line_start,
                &line_end,
            ],
        );
        if !self.seen_import_ids.insert(import_id.clone()) {
            return Ok(());
        }

        self.records.push(CodeImportRecord {
            repository_id: self.build.repository_id.clone(),
            source_scope: self.build.source_scope.clone(),
            import_id,
            file_id: self.file_id.to_owned(),
            path: self.path.to_owned(),
            module,
            target_hint: None,
            resolution_state: "unresolved".to_owned(),
            confidence_basis_points: 10_000,
            confidence_tier: "extracted".to_owned(),
            line_range: RepositoryCodeRange::new("line_range", range.line_start, range.line_end)
                .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        });

        Ok(())
    }

    fn push_script_line_imports(
        &mut self,
        language_id: &str,
        content: &str,
    ) -> Result<(), CodeIndexError> {
        for import in script_line_imports(language_id, content) {
            self.push_record(import.module, &import.range)?;
        }

        Ok(())
    }
}

struct ScriptLine<'a> {
    number: usize,
    byte_start: usize,
    byte_end: usize,
    text: &'a str,
}

struct ScriptLineImport {
    module: String,
    range: SyntaxRange,
}

fn script_line_imports(language_id: &str, content: &str) -> Vec<ScriptLineImport> {
    match language_id {
        "bash" => bash_line_imports(content),
        "ruby" => ruby_line_imports(content),
        _ => Vec::new(),
    }
}

fn ruby_line_imports(content: &str) -> Vec<ScriptLineImport> {
    source_lines(content)
        .into_iter()
        .filter_map(|line| {
            let statement = line.text.trim();
            ruby_import_statement(statement).then(|| ScriptLineImport {
                module: statement.to_owned(),
                range: line_range(&line, &line),
            })
        })
        .collect()
}

fn ruby_import_statement(statement: &str) -> bool {
    if statement.starts_with('#') || quoted_specifier(statement).is_none() {
        return false;
    }

    ["require", "require_relative", "load"]
        .iter()
        .any(|keyword| script_keyword_statement(statement, keyword))
}

fn bash_line_imports(content: &str) -> Vec<ScriptLineImport> {
    let lines = source_lines(content);
    let mut imports = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let statement = line.text.trim();
        if !bash_source_statement(statement) {
            continue;
        }
        let previous = previous_shellcheck_source_comment(&lines, index);
        let range_start = previous.unwrap_or(line);
        let module = previous.map_or_else(
            || statement.to_owned(),
            |comment| format!("{}\n{}", comment.text.trim(), statement),
        );
        imports.push(ScriptLineImport {
            module,
            range: line_range(range_start, line),
        });
    }

    imports
}

fn bash_source_statement(statement: &str) -> bool {
    script_keyword_statement(statement, "source")
        || statement
            .strip_prefix('.')
            .is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())
}

fn script_keyword_statement(statement: &str, keyword: &str) -> bool {
    statement
        .strip_prefix(keyword)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.starts_with('('))
}

fn previous_shellcheck_source_comment<'a>(
    lines: &'a [ScriptLine<'a>],
    index: usize,
) -> Option<&'a ScriptLine<'a>> {
    let previous = index.checked_sub(1).and_then(|index| lines.get(index))?;
    let text = previous.text.trim();

    (text.starts_with("# shellcheck ") && text.contains("source=")).then_some(previous)
}

fn source_lines(content: &str) -> Vec<ScriptLine<'_>> {
    let mut lines = Vec::new();
    let mut byte_start = 0usize;
    for (index, raw_line) in content.split_inclusive('\n').enumerate() {
        let without_lf = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let text = without_lf.strip_suffix('\r').unwrap_or(without_lf);
        lines.push(ScriptLine {
            number: index + 1,
            byte_start,
            byte_end: byte_start + text.len(),
            text,
        });
        byte_start += raw_line.len();
    }

    lines
}

fn line_range(start: &ScriptLine<'_>, end: &ScriptLine<'_>) -> SyntaxRange {
    SyntaxRange {
        byte_start: start.byte_start,
        byte_end: end.byte_end,
        line_start: start.number,
        line_end: end.number,
    }
}

fn javascript_like_dynamic_import(
    language_id: &str,
    content: &str,
    node: Node<'_>,
) -> Option<(String, SyntaxRange)> {
    if !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        || node.kind() != "call_expression"
    {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    if function.kind() != "import" {
        return None;
    }
    first_direct_string_argument(content, node.child_by_field_name("arguments")?)?;
    let source = javascript_like_dynamic_import_source(content, node);

    Some(source)
}

fn javascript_like_dynamic_import_source(content: &str, node: Node<'_>) -> (String, SyntaxRange) {
    let source_node = node
        .parent()
        .filter(|parent| parent.kind() == "await_expression")
        .unwrap_or(node);

    (
        compact_whitespace(&node_text(content, source_node)),
        syntax_range(source_node),
    )
}

fn first_direct_string_argument(content: &str, arguments: Node<'_>) -> Option<String> {
    for index in 0..arguments.named_child_count() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let child = arguments.named_child(index)?;
        if matches!(child.kind(), "string" | "string_literal") {
            return Some(node_text(content, child));
        }
        return None;
    }

    None
}

fn javascript_like_re_export(
    language_id: &str,
    content: &str,
    node: Node<'_>,
) -> Option<(String, SyntaxRange)> {
    if !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        || node.kind() != "export_statement"
    {
        return None;
    }
    let statement = compact_whitespace(&node_text(content, node));
    if !javascript_like_export_has_module_specifier(&statement) {
        return None;
    }

    Some((statement, syntax_range(node)))
}

fn javascript_like_export_has_module_specifier(statement: &str) -> bool {
    let Some(body) = statement
        .trim()
        .trim_end_matches(';')
        .trim()
        .strip_prefix("export ")
    else {
        return false;
    };
    body.rsplit_once(" from ")
        .is_some_and(|(_, module)| quoted_specifier(module).is_some())
}

fn quoted_specifier(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\''])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
}

fn go_import_specs(import_declaration: &str) -> Vec<String> {
    let mut specs = Vec::new();
    let mut search_start = 0usize;
    while let Some((quote_start, quote)) = next_go_import_quote(import_declaration, search_start) {
        let Some(quote_end) = import_declaration[quote_start + quote.len_utf8()..]
            .find(quote)
            .map(|offset| quote_start + quote.len_utf8() + offset)
        else {
            break;
        };
        let import_path = &import_declaration[quote_start + quote.len_utf8()..quote_end];
        if let Some(spec) =
            go_import_spec_before_quote(import_declaration, quote_start, import_path)
        {
            if !specs.contains(&spec) {
                specs.push(spec);
            }
        }
        search_start = quote_end + quote.len_utf8();
    }

    specs
}

fn next_go_import_quote(value: &str, start: usize) -> Option<(usize, char)> {
    let mut line_comment = false;
    let mut block_comment = false;
    let mut characters = value[start..].char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        let index = start + offset;
        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_comment {
            if character == '*' && value[index + character.len_utf8()..].starts_with('/') {
                characters.next();
                block_comment = false;
            }
            continue;
        }
        if character == '/' && value[index + character.len_utf8()..].starts_with('/') {
            characters.next();
            line_comment = true;
            continue;
        }
        if character == '/' && value[index + character.len_utf8()..].starts_with('*') {
            characters.next();
            block_comment = true;
            continue;
        }
        if matches!(character, '"' | '`') {
            return Some((index, character));
        }
    }

    None
}

fn go_import_spec_before_quote(
    import_declaration: &str,
    quote_start: usize,
    import_path: &str,
) -> Option<String> {
    if import_path.trim().is_empty() {
        return None;
    }
    let prefix_start = import_declaration[..quote_start]
        .rfind(['\n', '(', ';'])
        .map_or(0, |index| index + 1);
    let raw_prefix = import_declaration[prefix_start..quote_start].trim();
    if raw_prefix.contains("//")
        || raw_prefix.starts_with("/*")
        || raw_prefix.rfind("/*") > raw_prefix.rfind("*/")
    {
        return None;
    }
    let prefix = raw_prefix
        .strip_prefix("import")
        .map_or(raw_prefix, str::trim);
    let alias = prefix
        .split_whitespace()
        .last()
        .filter(|value| matches!(*value, "." | "_") || go_identifier(value));

    Some(match alias {
        Some(alias) => format!("{alias} {import_path}"),
        None => import_path.to_owned(),
    })
}

fn go_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_import_node(language_id: &str, node: Node<'_>) -> bool {
    match node.kind() {
        "import" => {
            node.is_named() && !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        }
        "import_declaration"
        | "import_from_statement"
        | "import_statement"
        | "namespace_use_declaration"
        | "preproc_include"
        | "use_declaration"
        | "using_declaration"
        | "using_directive" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::go_import_specs;

    #[test]
    fn go_import_specs_ignore_quotes_inside_multiline_comments() {
        let specs = go_import_specs(
            r#"
import (
    "context"
    /*
       alias "example.com/commented"
       "example.com/also-commented"
    */
    named "example.com/used"
)
"#,
        );

        assert_eq!(specs, ["context", "named example.com/used"]);
    }
}
