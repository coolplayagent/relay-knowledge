use crate::code::parser::imports::{ScriptLineImport, line_range, quoted_specifier, source_lines};

pub(in crate::code::parser) fn line_imports(content: &str) -> Vec<ScriptLineImport> {
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
        .any(|keyword| ruby_keyword_statement(statement, keyword))
}

fn ruby_keyword_statement(statement: &str, keyword: &str) -> bool {
    statement
        .strip_prefix(keyword)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.starts_with('('))
}
