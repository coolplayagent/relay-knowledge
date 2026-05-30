use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::{
    code::configuration,
    domain::{CodeImportRecord, RepositoryCodeRange},
};

use super::{
    super::{CodeIndexError, SnapshotBuild, stable_id},
    languages,
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
        if let Some((module, range)) =
            languages::javascript_like_dynamic_import(language_id, content, node)
        {
            imports.push_record(module, &range)?;
        } else if let Some((module, range)) =
            languages::javascript_like_re_export(language_id, content, node)
        {
            imports.push_record(module, &range)?;
        } else if language_id == "markdown" {
            imports.push_markdown_imports(content, node)?;
        } else if is_import_node(language_id, node) {
            imports.push_records(language_id, content, node)?;
        }
        push_children_reverse(node, &mut stack);
    }
    imports.push_line_imports(language_id, content)?;
    imports.push_configuration_imports(language_id, content)?;

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
            languages::go::import_specs(&module_text)
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

    fn push_markdown_imports(
        &mut self,
        content: &str,
        node: Node<'_>,
    ) -> Result<(), CodeIndexError> {
        for (module, range) in languages::markdown::imports(content, node) {
            self.push_record(module, &range)?;
        }

        Ok(())
    }

    fn push_line_imports(
        &mut self,
        language_id: &str,
        content: &str,
    ) -> Result<(), CodeIndexError> {
        let imports = match language_id {
            "bash" => languages::bash::line_imports(content),
            "ruby" => languages::ruby::line_imports(content),
            _ => Vec::new(),
        };
        for import in imports {
            self.push_record(import.module, &import.range)?;
        }

        Ok(())
    }

    fn push_configuration_imports(
        &mut self,
        language_id: &str,
        content: &str,
    ) -> Result<(), CodeIndexError> {
        for import in configuration::structured_imports(self.path, language_id, content) {
            let range = SyntaxRange {
                byte_start: import.range.byte_start,
                byte_end: import.range.byte_end,
                line_start: import.range.line_start,
                line_end: import.range.line_end,
            };
            self.push_record(import.module, &range)?;
        }

        Ok(())
    }
}

pub(in crate::code::parser) struct ScriptLine<'a> {
    pub(in crate::code::parser) number: usize,
    pub(in crate::code::parser) byte_start: usize,
    pub(in crate::code::parser) byte_end: usize,
    pub(in crate::code::parser) text: &'a str,
}

pub(in crate::code::parser) struct ScriptLineImport {
    pub(in crate::code::parser) module: String,
    pub(in crate::code::parser) range: SyntaxRange,
}

pub(in crate::code::parser) fn source_lines(content: &str) -> Vec<ScriptLine<'_>> {
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

pub(in crate::code::parser) fn line_range(
    start: &ScriptLine<'_>,
    end: &ScriptLine<'_>,
) -> SyntaxRange {
    SyntaxRange {
        byte_start: start.byte_start,
        byte_end: end.byte_end,
        line_start: start.number,
        line_end: end.number,
    }
}

pub(in crate::code::parser) fn quoted_specifier(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\''])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
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
