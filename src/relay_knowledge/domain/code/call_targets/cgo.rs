//! Selects C linkage targets once per name, including inherited internal linkage.

use std::collections::BTreeSet;

use super::{CgoTarget, callable_definition_symbol};

pub(crate) fn select_cgo_target<'a, T: 'a>(
    symbols: impl Iterator<Item = (&'a T, &'a str, &'a str, &'a str, &'a str)> + Clone,
) -> CgoTarget<&'a T> {
    // A definition inherits a prior static declaration in its translation unit.
    // Index traversal order must not change that linkage evidence.
    let internal_paths = symbols
        .clone()
        .filter(|(_, language, kind, signature, _)| {
            *language == "c"
                && matches!(*kind, "function" | "function_declaration")
                && !proven_external_linkage(signature)
        })
        .map(|(_, _, _, _, path)| path)
        .collect::<BTreeSet<_>>();
    let mut declaration = None;
    let mut declaration_count = 0u8;
    let mut definition = None;
    for (symbol, language, kind, signature, path) in symbols {
        if language != "c"
            || !matches!(kind, "function" | "function_declaration")
            || internal_paths.contains(path)
        {
            continue;
        }
        if callable_definition_symbol(kind, signature) {
            if definition.is_some() {
                return CgoTarget::Ambiguous;
            }
            definition = Some(symbol);
        } else {
            declaration = Some(symbol);
            declaration_count = declaration_count.saturating_add(1).min(2);
        }
    }
    if let Some(symbol) = definition {
        return CgoTarget::Unique(symbol);
    }
    match (declaration_count, declaration) {
        (0, _) => CgoTarget::Missing,
        (1, Some(symbol)) => CgoTarget::Unique(symbol),
        _ => CgoTarget::Ambiguous,
    }
}

fn proven_external_linkage(signature: &str) -> bool {
    let bytes = signature.as_bytes();
    let mut position = 0;
    let mut depth = 0usize;
    while position < bytes.len() {
        let rest = &bytes[position..];
        if rest.starts_with(b"/*") {
            position += rest
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(rest.len(), |end| end + 2);
            continue;
        }
        if rest.starts_with(b"//") {
            // Display signatures collapse source lines, potentially hiding a
            // later storage class inside a line comment.
            return false;
        }
        match bytes[position] {
            b'"' | b'\'' => {
                let quote = bytes[position];
                position += 1;
                while position < bytes.len() {
                    let byte = bytes[position];
                    position += 1;
                    if byte == b'\\' {
                        position = (position + 1).min(bytes.len());
                    } else if byte == quote {
                        break;
                    }
                }
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'{' | b';' if depth == 0 => return true,
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = position;
                position += 1;
                while position < bytes.len()
                    && (bytes[position].is_ascii_alphanumeric() || bytes[position] == b'_')
                {
                    position += 1;
                }
                if depth == 0 && &bytes[start..position] == b"static" {
                    return false;
                }
                continue;
            }
            _ => {}
        }
        position += 1;
    }
    // A bounded excerpt without a terminator cannot prove external linkage.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgo_linkage_ignores_parameters_bodies_comments_and_attribute_strings() {
        for signature in [
            "int decode(int a[static 1]) {",
            "int decode(void) { static int cache; }",
            "/* static */ int decode(void);",
            "__attribute__((annotate(\"static \\\" marker\"))) int decode(void);",
        ] {
            assert!(proven_external_linkage(signature), "{signature}");
        }
        assert!(!proven_external_linkage(
            "__attribute__((unused)) static int decode(void);"
        ));
        assert!(!proven_external_linkage("static inline int decode(void) {"));
        for signature in [
            "int // note static decode(void) {",
            "__attribute__((annotate(\"truncated",
            "int decode(",
            "int /* incomplete",
        ] {
            assert!(!proven_external_linkage(signature), "{signature}");
        }
    }

    #[test]
    fn cgo_linkage_is_inherited_within_a_file_without_hiding_other_files() {
        let symbols = [
            (1, "c", "function", "int decode(void) {", "private.c"),
            (
                2,
                "c",
                "function_declaration",
                "static int decode(void);",
                "private.c",
            ),
            (
                3,
                "c",
                "function",
                "int decode(int a[static 1]) {",
                "public.c",
            ),
        ];
        let tuples = || {
            symbols
                .iter()
                .map(|(id, lang, kind, sig, path)| (id, *lang, *kind, *sig, *path))
        };
        assert_eq!(select_cgo_target(tuples().take(2)), CgoTarget::Missing);
        assert_eq!(select_cgo_target(tuples()), CgoTarget::Unique(&3));
    }
}
