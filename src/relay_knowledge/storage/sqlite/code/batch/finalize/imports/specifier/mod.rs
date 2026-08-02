//! Extracts bounded quoted module specifiers from import statements.

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(super) fn quoted(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\''])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
}
