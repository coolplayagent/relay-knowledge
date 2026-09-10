//! Normalize standard builtin specifiers without compiler-dependent width guesses.
use super::budget::Budget;
use tree_sitter::Node;

pub(super) fn canonical(
    content: &str,
    node: Node<'_>,
    budget: &mut Budget<'_>,
) -> Option<&'static str> {
    let mut base = None;
    let mut signed = false;
    let mut unsigned = false;
    let mut short = false;
    let mut longs = 0;
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        budget.spend()?;
        if node.kind() == "comment" {
            continue;
        }
        if node.is_error() || node.is_missing() {
            return None;
        }
        if node.child_count() != 0 {
            if !matches!(node.kind(), "primitive_type" | "sized_type_specifier")
                || stack.len().checked_add(node.child_count())? > budget.remaining.min(*budget.file)
            {
                return None;
            }
            for index in 0..node.child_count() {
                stack.push(node.child(u32::try_from(index).ok()?)?);
            }
            continue;
        }
        if node.is_named() && node.kind() != "primitive_type" {
            return None;
        }
        match node.utf8_text(content.as_bytes()).ok()? {
            "signed" if !signed && !unsigned => signed = true,
            "unsigned" if !signed && !unsigned => unsigned = true,
            "short" if !short => short = true,
            "long" if longs < 2 => longs += 1,
            token => {
                let primitive = match token {
                    "int" => "int",
                    "char" => "char",
                    "float" => "float",
                    "double" => "double",
                    "void" => "void",
                    "bool" => "bool",
                    "_Bool" => "_Bool",
                    "wchar_t" => "wchar_t",
                    "char8_t" => "char8_t",
                    "char16_t" => "char16_t",
                    "char32_t" => "char32_t",
                    _ => return None,
                };
                if base.replace(primitive).is_some() {
                    return None;
                }
            }
        }
    }
    // Plain char, signed char and unsigned char are distinct even on targets
    // where their representations coincide. Likewise never fold int into long.
    match (base.unwrap_or("int"), short, longs) {
        ("int", false, 0) => Some(if unsigned { "unsigned int" } else { "int" }),
        ("int", true, 0) => Some(if unsigned {
            "unsigned short int"
        } else {
            "short int"
        }),
        ("int", false, 1) => Some(if unsigned {
            "unsigned long int"
        } else {
            "long int"
        }),
        ("int", false, 2) => Some(if unsigned {
            "unsigned long long int"
        } else {
            "long long int"
        }),
        ("char", false, 0) => Some(if unsigned {
            "unsigned char"
        } else if signed {
            "signed char"
        } else {
            "char"
        }),
        ("double", false, 1) if !signed && !unsigned => Some("long double"),
        (base, false, 0) if !signed && !unsigned => Some(base),
        _ => None,
    }
}
