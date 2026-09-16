//! C++ template identities from declaration parameters and structured arguments.
use super::*;

/// Bare named type arguments require C++ lookup evidence beyond their spelling.
/// Keep their detached owner unresolved instead of equating ns::V with ::V.
pub(super) fn arguments_proven(name: &str) -> bool {
    let Some((_, tail)) = name.split_once('<') else {
        return true;
    };
    let Some((arguments, _)) = tail.rsplit_once('>') else {
        return false;
    };
    arguments.split(',').all(|argument| {
        let argument = argument.trim();
        argument
            .strip_prefix('@')
            .is_some_and(|slot| slot.parse::<usize>().is_ok())
            || argument.parse::<i128>().is_ok()
            || matches!(argument, "true" | "false")
            || (!argument.is_empty()
                && argument.split_whitespace().all(|word| {
                    matches!(
                        word,
                        "void"
                            | "bool"
                            | "char"
                            | "char8_t"
                            | "char16_t"
                            | "char32_t"
                            | "wchar_t"
                            | "short"
                            | "int"
                            | "long"
                            | "float"
                            | "double"
                            | "signed"
                            | "unsigned"
                    )
                }))
    })
}

pub(super) fn name(node: Node<'_>, context: Node<'_>, source: &str) -> Option<String> {
    let parameters = parameters(context, source)?;
    let base = node.child_by_field_name("name").unwrap_or(node);
    let base = base.utf8_text(source.as_bytes()).ok()?.trim();
    if base.is_empty() || !base.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return Some(if parameters.is_empty() {
            base.to_owned()
        } else {
            format!(
                "{base}<{}>",
                (0..parameters.len())
                    .map(|i| format!("@{i}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });
    };
    if arguments.named_child_count() > 16 {
        return None;
    }
    let mut cursor = arguments.walk();
    let mut values = Vec::new();
    for argument in arguments.named_children(&mut cursor) {
        let raw = argument.utf8_text(source.as_bytes()).ok()?.trim();
        if raw.len() > 256 || raw.contains(['/', '\n']) {
            return None;
        }
        // A parameter argument binds to its declaration slot; concrete arguments
        // preserve their spelling rather than collapsing specializations together.
        let value = if let Some(index) = parameters.iter().position(|p| p == raw) {
            format!("@{index}")
        } else {
            raw.split_whitespace().collect::<Vec<_>>().join(" ")
        };
        values.push(value);
    }
    Some(format!("{base}<{}>", values.join(",")))
}

fn parameters(mut context: Node<'_>, source: &str) -> Option<Vec<String>> {
    let mut owner_template = None;
    for _ in 0..16 {
        let Some(parent) = context.parent() else {
            break;
        };
        if matches!(
            parent.kind(),
            "translation_unit" | "namespace_definition" | "class_specifier" | "struct_specifier"
        ) {
            break;
        }
        if parent.kind() == "template_declaration" {
            // Out-of-class member templates have consecutive template layers:
            // the outer layer binds the owner, the inner layer the method.
            owner_template = Some(parent);
        }
        context = parent;
    }
    let Some(owner_template) = owner_template else {
        return Some(Vec::new());
    };
    let list = owner_template.child_by_field_name("parameters")?;
    if list.named_child_count() > 16 {
        return None;
    }
    let mut cursor = list.walk();
    list.named_children(&mut cursor)
        .map(|parameter| {
            if parameter.kind() != "type_parameter_declaration"
                || parameter.named_child_count() != 1
            {
                return None;
            }
            Some(
                parameter
                    .named_child(0)?
                    .utf8_text(source.as_bytes())
                    .ok()?
                    .to_owned(),
            )
        })
        .collect()
}
