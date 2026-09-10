//! Static-import method shadowing is limited to lexical owners and applicable signatures.
use super::names::text;
use tree_sitter::Node;
pub(super) fn shadows(method: Node<'_>, call: Node<'_>, content: &str) -> bool {
    let mut owner = method.parent();
    while let Some(node) = owner {
        if matches!(
            node.kind(),
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
        ) {
            if call.start_byte() < node.start_byte() || call.end_byte() > node.end_byte() {
                return false;
            }
            break;
        }
        owner = node.parent();
    }
    let Some(params) = method.child_by_field_name("parameters") else {
        return false;
    };
    let Some(args) = call.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = params.walk();
    let params = params.named_children(&mut cursor).collect::<Vec<_>>();
    let varargs = params
        .last()
        .is_some_and(|p| p.kind() == "spread_parameter");
    if (!varargs && params.len() != args.named_child_count())
        || (varargs && args.named_child_count() < params.len().saturating_sub(1))
    {
        return false;
    }
    for (index, param) in params.iter().enumerate() {
        let Ok(argument_index) = u32::try_from(index) else {
            return true;
        };
        if param.kind() == "spread_parameter" {
            break;
        }
        if args
            .named_child(argument_index)
            .is_some_and(|arg| arg.kind() == "string_literal")
        {
            if let Some(ty) = param.child_by_field_name("type") {
                let ty = text(ty, content);
                if matches!(
                    ty,
                    "int" | "long" | "boolean" | "float" | "double" | "byte" | "short" | "char"
                ) || ty.ends_with("[]")
                {
                    return false;
                }
            }
        }
    }
    true
}
