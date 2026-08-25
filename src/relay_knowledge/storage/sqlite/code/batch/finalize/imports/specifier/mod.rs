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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CIncludeDelimiter {
    Quoted,
    Angle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CIncludeSpecifier<'a> {
    pub(super) target: &'a str,
    pub(super) delimiter: CIncludeDelimiter,
}

pub(super) fn c_include_specifier(statement: &str) -> Option<CIncludeSpecifier<'_>> {
    let rest = statement.trim().strip_prefix("#include")?.trim_start();
    let (closing_delimiter, delimiter) = match rest.as_bytes().first()? {
        b'"' => ('"', CIncludeDelimiter::Quoted),
        b'<' => ('>', CIncludeDelimiter::Angle),
        _ => return None,
    };
    let body = &rest[1..];
    let end = body.find(closing_delimiter)?;

    Some(CIncludeSpecifier {
        target: &body[..end],
        delimiter,
    })
}
