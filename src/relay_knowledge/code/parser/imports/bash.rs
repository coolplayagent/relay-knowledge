use super::{ScriptLine, ScriptLineImport, line_range, source_lines};

pub(super) fn line_imports(content: &str) -> Vec<ScriptLineImport> {
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
    bash_keyword_statement(statement, "source")
        || statement
            .strip_prefix('.')
            .is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())
}

fn bash_keyword_statement(statement: &str, keyword: &str) -> bool {
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
