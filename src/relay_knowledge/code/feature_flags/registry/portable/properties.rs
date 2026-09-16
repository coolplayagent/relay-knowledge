//! Explicit property getters; ordinary member access never invokes a method.
use super::*;

pub(super) fn getter<'a>(
    node: Node<'a>,
    language: &str,
    source: &str,
) -> Option<(String, Node<'a>)> {
    if syntax::is_function(node.kind()) && is_property_method(node, language, source) {
        return Some((
            syntax::function_name(node, source)?,
            syntax::single_return(node, language)?,
        ));
    }
    if node.kind() != "property_declaration" {
        return None;
    }
    let name = node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|n| n.kind() == "variable_declaration")
            .and_then(|n| n.child_by_field_name("name").or_else(|| n.named_child(0)))
    })?;
    let name = syntax::text(name, source).to_owned();
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).take(64).collect();
    let mut candidate = children.iter().copied().find(|n| {
        matches!(
            n.kind(),
            "arrow_expression_clause" | "accessor_list" | "computed_property" | "getter"
        )
    })?;
    for _ in 0..16 {
        if syntax::is_read_candidate(candidate.kind()) {
            return Some((name, candidate));
        }
        if !matches!(
            candidate.kind(),
            "arrow_expression_clause"
                | "accessor_list"
                | "accessor_declaration"
                | "computed_property"
                | "getter"
                | "block"
                | "statements"
                | "return_statement"
                | "control_transfer_statement"
                | "function_body"
        ) {
            return None;
        }
        let mut cursor = candidate.walk();
        let mut children = candidate
            .named_children(&mut cursor)
            .filter(|n| !n.is_extra());
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        candidate = child;
    }
    None
}

pub(super) fn is_property_method(node: Node<'_>, language: &str, source: &str) -> bool {
    if language == "python" {
        let Some(decorated) = node.parent().filter(|n| n.kind() == "decorated_definition") else {
            return false;
        };
        let mut cursor = decorated.walk();
        return decorated
            .named_children(&mut cursor)
            .any(|n| n.kind() == "decorator" && syntax::text(n, source) == "@property");
    }
    matches!(language, "javascript" | "jsx" | "typescript" | "tsx")
        && node.kind() == "method_definition"
        && node.child(0).is_some_and(|n| n.kind() == "get")
}

impl Analysis<'_> {
    pub(super) fn property_value(
        &self,
        use_site: Node<'_>,
        name: &str,
        receiver: Option<&str>,
        depth: usize,
        visiting: &mut BTreeSet<usize>,
    ) -> Option<Value> {
        if receiver.is_none()
            && (matches!(
                self.input.language_id,
                "javascript" | "jsx" | "typescript" | "tsx" | "python" | "php"
            ) || scopes::parameter_shadows(use_site, name, self.input.content)
                || self.declarations.get(name).is_some_and(|nodes| {
                    nodes.iter().any(|node| {
                        getter(*node, self.input.language_id, self.input.content).is_none()
                            && syntax::visible_function(*node, use_site, name, self.input.content)
                    })
                }))
        {
            return None;
        }
        if receiver.is_some_and(|r| !matches!(r, "self" | "this")) {
            return None;
        }
        let candidates = self.properties.get(name)?;
        let call = receiver.map_or_else(|| name.to_owned(), |r| format!("{r}.{name}"));
        let mut candidates = candidates.iter().filter(|(owner, _)| {
            syntax::visible_function(*owner, use_site, &call, self.input.content)
        });
        let (owner, value) = candidates.next()?;
        if candidates.next().is_some() || !self.stable_getter(*owner, name) {
            return None;
        }
        let mut result = self.evaluate(*value, depth + 1, visiting)?;
        if let Value::Read(ref mut read) = result {
            read.start = use_site.start_byte();
            read.end = use_site.end_byte();
        }
        Some(result)
    }
}
