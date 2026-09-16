//! Preserve statically typed string compositions until their imported constants arrive.
use super::*;

impl Analysis<'_> {
    pub(super) fn string_expression(
        &self,
        node: Node<'_>,
        depth: usize,
        visiting: &mut BTreeSet<usize>,
    ) -> Option<Vec<CodeConfigStringPart>> {
        if depth >= MAX_DEPTH || node.has_error() || !visiting.insert(node.id()) {
            return None;
        }
        self.evaluations
            .set(self.evaluations.get().saturating_add(1));
        if self.evaluations.get() >= MAX_EVALUATIONS {
            return None;
        }
        let result = self.string_components(node, depth, visiting);
        visiting.remove(&node.id());
        result
    }

    fn string_components(
        &self,
        node: Node<'_>,
        depth: usize,
        visiting: &mut BTreeSet<usize>,
    ) -> Option<Vec<CodeConfigStringPart>> {
        if let Some(Value::Literal(value)) = self.evaluate(node, depth + 1, &mut BTreeSet::new()) {
            return (value.kind == "string")
                .then(|| vec![CodeConfigStringPart::Literal(value.text)]);
        }
        if node.kind() == "parenthesized_expression" && node.named_child_count() == 1 {
            return self.string_expression(node.named_child(0)?, depth + 1, visiting);
        }
        if node.kind() == "binary_expression" {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            let operator = node.child_by_field_name("operator")?;
            if syntax::text(operator, self.input.content) != "+" {
                return None;
            }
            let mut parts = self.string_expression(left, depth + 1, visiting)?;
            parts.extend(self.string_expression(right, depth + 1, visiting)?);
            return (parts.len() <= 32).then_some(parts);
        }
        if !syntax::is_identifier(node.kind()) {
            return None;
        }
        let name = syntax::text(node, self.input.content);
        if scopes::parameter_shadows(node, name, self.input.content) {
            return None;
        }
        let visible = self
            .declarations
            .get(name)
            .into_iter()
            .flatten()
            .filter(|candidate| {
                syntax::visible(**candidate, node)
                    || (self.input.language_id == "go" && self.stable_constant(**candidate, name))
            })
            .copied()
            .collect::<Vec<_>>();
        let reference = match visible.as_slice() {
            [declaration] if self.stable_constant(*declaration, name) => {
                let (_, value) = syntax::assignment(*declaration, self.input.content)?;
                self.string_expression(value, depth + 1, visiting)?;
                self.binding(*declaration, name)
            }
            [] => imports::imported_binding(self, name, node)?,
            _ => return None,
        };
        Some(vec![CodeConfigStringPart::Reference(reference)])
    }
}
