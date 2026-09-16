//! Conservative expression evaluation; unknown syntax never becomes a guessed value.
use super::*;

#[derive(Clone)]
pub(super) enum Atom {
    Literal(String),
    Reference(String),
    Expression(Vec<CodeConfigStringPart>),
}
#[derive(Clone)]
pub(super) struct Literal {
    pub text: String,
    pub kind: String,
}
#[derive(Clone)]
pub(super) struct Read {
    pub nullable: bool,
    pub key: Atom,
    pub namespace: String,
    pub default: Option<Literal>,
    pub value_type: Option<String>,
    pub start: usize,
    pub end: usize,
    pub incomplete: Option<String>,
}
#[derive(Clone)]
pub(super) enum Value {
    Literal(Literal),
    Read(Read),
}

impl Analysis<'_> {
    pub(super) fn evaluate(
        &self,
        node: Node<'_>,
        depth: usize,
        visiting: &mut BTreeSet<usize>,
    ) -> Option<Value> {
        self.evaluations
            .set(self.evaluations.get().saturating_add(1));
        if self.evaluations.get() >= MAX_EVALUATIONS
            || depth >= MAX_DEPTH
            || !visiting.insert(node.id())
        {
            return None;
        }
        let result = self.evaluate_inner(node, depth, visiting);
        visiting.remove(&node.id());
        result
    }

    fn evaluate_inner(
        &self,
        node: Node<'_>,
        depth: usize,
        visiting: &mut BTreeSet<usize>,
    ) -> Option<Value> {
        if node.has_error() {
            return None;
        }
        let text = syntax::text(node, self.input.content);
        if let Some(literal) = literal(node, text, self.input.language_id) {
            return Some(Value::Literal(literal));
        }
        if matches!(
            node.kind(),
            "parenthesized_expression"
                | "expression_statement"
                | "return_statement"
                | "return_expression"
        ) && node.named_child_count() == 1
        {
            return self.evaluate(node.named_child(0)?, depth + 1, visiting);
        }
        if matches!(
            node.kind(),
            "binary_expression"
                | "boolean_operator"
                | "binary_operator"
                | "additive_expression"
                | "elvis_expression"
                | "infix_expression"
                | "nil_coalescing_expression"
        ) {
            let left = node
                .child_by_field_name("left")
                .or_else(|| node.named_child(0))?;
            let right = node
                .child_by_field_name("right")
                .or_else(|| node.named_child(1))?;
            let op = self
                .input
                .content
                .get(left.end_byte()..right.start_byte())?
                .trim();
            let fallback = matches!(
                (self.input.language_id, op),
                (
                    "javascript" | "jsx" | "typescript" | "tsx" | "swift" | "csharp",
                    "??"
                ) | ("kotlin", "?:")
                    | ("ruby", "||")
                    | ("python", "or")
            );
            if fallback {
                let Value::Read(mut read) = self.evaluate(left, depth + 1, visiting)? else {
                    return None;
                };
                if read.namespace.is_empty() && matches!(read.key, Atom::Reference(_)) {
                    read.default = None;
                    read.incomplete = Some("unproven_getter_fallback".into());
                    return Some(Value::Read(read));
                }
                if matches!(op, "??" | "?:") && !read.nullable {
                    return Some(Value::Read(read));
                }
                if read.default.is_none() {
                    read.default = match self.evaluate(right, depth + 1, visiting) {
                        Some(Value::Literal(value)) => Some(value),
                        _ => None,
                    };
                    if read.default.is_none() {
                        read.incomplete = Some("unevaluated_explicit_default".into());
                    }
                } else if matches!(op, "||" | "or") {
                    // Truth-dependent fallbacks can replace an existing false/empty default.
                    read.default = None;
                    read.incomplete = Some("truth_dependent_default".into());
                }
                return Some(Value::Read(read));
            }
            if op != "+" && !(self.input.language_id == "php" && op == ".") {
                return None;
            }
            let (Value::Literal(a), Value::Literal(b)) = (
                self.evaluate(left, depth + 1, visiting)?,
                self.evaluate(right, depth + 1, visiting)?,
            ) else {
                return None;
            };
            if a.kind != "string" || b.kind != "string" || a.text.len() + b.text.len() > 4096 {
                return None;
            }
            return Some(Value::Literal(Literal {
                text: a.text + &b.text,
                kind: "string".into(),
            }));
        }
        if syntax::is_identifier(node.kind()) {
            let name = text.trim_start_matches('$');
            if let Some(value) = self.property_value(node, name, None, depth, visiting) {
                return Some(value);
            }
            return self
                .reaching_value(node, name)
                .and_then(|value| self.evaluate(value, depth + 1, visiting));
        }
        let call = syntax::call(node, self.input.content)?;
        if let Some(namespace) = syntax::reader_namespace(self.input.language_id, &call.name) {
            let shadowed = self.reader_import_shadowed(&call.name, node)
                || syntax::shadowed_reader(
                    &call.name,
                    node,
                    &self.declarations,
                    &self.functions,
                    self.input.content,
                );
            let object_reader = call
                .name
                .rsplit_once('.')
                .is_some_and(|(receiver, method)| {
                    crate::code::feature_flags::extractors::is_config_reader(receiver, method)
                });
            let contextual_environment =
                self.input.language_id == "starlark" && call.name.ends_with(".getenv");
            if shadowed && !object_reader && !contextual_environment {
                return None;
            }
            if call.arguments.len() > 2 {
                return None;
            }
            let key_node = if call.literal_key.is_some() {
                node
            } else {
                *call.arguments.first()?
            };
            let imported =
                imports::imported_binding(self, syntax::text(key_node, self.input.content), node);
            let key = if let Some(key) = &call.literal_key {
                Atom::Literal(key.clone())
            } else {
                match self.evaluate(key_node, depth + 1, visiting) {
                    Some(Value::Literal(value)) if value.kind == "string" => {
                        Atom::Literal(value.text)
                    }
                    _ if self
                        .string_expression(key_node, depth + 1, &mut BTreeSet::new())
                        .is_some() =>
                    {
                        let parts =
                            self.string_expression(key_node, depth + 1, &mut BTreeSet::new())?;
                        if let [CodeConfigStringPart::Reference(reference)] = parts.as_slice() {
                            Atom::Reference(reference.clone())
                        } else {
                            Atom::Expression(parts)
                        }
                    }
                    _ => Atom::Reference(imported.clone().unwrap_or_else(|| {
                        self.binding(key_node, syntax::text(key_node, self.input.content))
                    })),
                }
            };
            let default_node = syntax::default_argument(self.input.language_id, &call);
            let default = default_node
                .and_then(|n| self.evaluate(*n, depth + 1, visiting))
                .and_then(|value| {
                    if let Value::Literal(lit) = value {
                        Some(lit)
                    } else {
                        None
                    }
                });
            let incomplete = if contextual_environment {
                Some("unproven_environment_receiver".into())
            } else if shadowed {
                Some("unproven_configuration_receiver".into())
            } else if matches!(key, Atom::Reference(_))
                && imported.is_none()
                && self
                    .string_expression(key_node, depth + 1, &mut BTreeSet::new())
                    .is_none()
            {
                Some("unresolved_configuration_key".into())
            } else if default_node.is_some() && default.is_none() {
                Some("unevaluated_explicit_default".into())
            } else {
                None
            };
            return Some(Value::Read(Read {
                nullable: true,
                key,
                namespace: namespace.into(),
                value_type: default.as_ref().map(|d| d.kind.clone()),
                default,
                start: node.start_byte(),
                end: node.end_byte(),
                incomplete,
            }));
        }
        if let Some(name) = &call.literal_key {
            return self.property_value(node, name, Some(&call.name), depth, visiting);
        }
        if let Some(value) = self.converted_call(&call, node, depth, visiting) {
            return Some(value);
        }
        if call.arguments.is_empty() {
            if self.binding_shadowed(node, &call.name) {
                return None;
            }
            let leaf = call.name.rsplit('.').next()?;
            let Some(candidates) = self.functions.get(leaf) else {
                let reference = imports::imported_binding(self, &call.name, node)?;
                return Some(Value::Read(Read {
                    nullable: true,
                    key: Atom::Reference(reference),
                    namespace: String::new(),
                    default: None,
                    value_type: None,
                    start: node.start_byte(),
                    end: node.end_byte(),
                    incomplete: None,
                }));
            };
            let candidates = candidates
                .iter()
                .copied()
                .filter(|f| syntax::visible_function(*f, node, &call.name, self.input.content))
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                return None;
            }
            let function = candidates[0];
            if !syntax::zero_arguments(function, self.input.content)
                || !self.stable_getter(function, leaf)
            {
                return None;
            }
            let value = syntax::single_return(function, self.input.language_id)?;
            let mut value = self.evaluate(value, depth + 1, visiting)?;
            if let Value::Read(ref mut read) = value {
                read.start = node.start_byte();
                read.end = node.end_byte();
            }
            return Some(value);
        }
        None
    }
}

fn literal(node: Node<'_>, text: &str, language: &str) -> Option<Literal> {
    if text.len() > 4096 {
        return None;
    }
    let kind = node.kind();
    if matches!(kind, "true" | "false" | "boolean_literal" | "boolean") {
        return Some(Literal {
            text: text.to_lowercase(),
            kind: "boolean".into(),
        });
    }
    if matches!(
        kind,
        "integer"
            | "integer_literal"
            | "int_literal"
            | "number"
            | "float"
            | "float_literal"
            | "real_literal"
    ) && text.parse::<f64>().is_ok()
    {
        return Some(Literal {
            text: text.to_owned(),
            kind: "number".into(),
        });
    }
    if !matches!(
        kind,
        "string"
            | "string_literal"
            | "interpreted_string_literal"
            | "raw_string_literal"
            | "string_value"
            | "line_string_literal"
            | "encapsed_string"
            | "string_content"
            | "template_string"
    ) {
        return None;
    }
    strings::decode(text, language).map(|text| Literal {
        text,
        kind: "string".into(),
    })
}
