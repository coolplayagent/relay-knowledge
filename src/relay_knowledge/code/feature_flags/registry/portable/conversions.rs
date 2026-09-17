//! Proven built-in conversions; arbitrary wrappers never supply runtime values.
use super::*;

impl Analysis<'_> {
    pub(super) fn converted_call(
        &self,
        call: &syntax::Call<'_>,
        node: Node<'_>,
        depth: usize,
        visiting: &mut BTreeSet<usize>,
    ) -> Option<Value> {
        if self.input.language_id == "rust" {
            let receiver = node
                .child_by_field_name("function")?
                .child_by_field_name("value");
            if let Some(receiver) = receiver {
                let method = call.name.rsplit('.').next()?;
                if method == "unwrap_or" && call.arguments.len() == 1 {
                    let Value::Read(mut read) = self.evaluate(receiver, depth + 1, visiting)?
                    else {
                        return None;
                    };
                    read.default = match self.evaluate(call.arguments[0], depth + 1, visiting) {
                        Some(Value::Literal(value)) => Some(value),
                        _ => None,
                    };
                    if read.default.is_none() {
                        read.incomplete = Some("unevaluated_explicit_default".into());
                    }
                    return Some(Value::Read(read));
                }
                if matches!(method, "to_owned" | "to_string") && call.arguments.is_empty() {
                    let value = self.evaluate(receiver, depth + 1, visiting)?;
                    return matches!(&value,Value::Literal(lit) if lit.kind=="string")
                        .then_some(value);
                }
            }
        }
        let kind = match (self.input.language_id, call.name.as_str()) {
            ("python" | "starlark", "bool")
            | ("javascript" | "jsx" | "typescript" | "tsx", "Boolean")
            | ("php", "boolval")
            | ("csharp", "bool.Parse" | "Boolean.Parse" | "System.Boolean.Parse") => "boolean",
            ("python" | "starlark", "int" | "float")
            | ("javascript" | "jsx" | "typescript" | "tsx", "Number")
            | ("php", "intval" | "floatval")
            | ("csharp", "int.Parse" | "Int32.Parse" | "System.Int32.Parse") => "number",
            _ => return None,
        };
        if call.arguments.len() != 1
            || self.reader_import_shadowed(&call.name, node)
            || syntax::shadowed_reader(
                &call.name,
                node,
                &self.declarations,
                &self.functions,
                self.input.content,
            )
        {
            return None;
        }
        let value = self.evaluate(call.arguments[0], depth + 1, visiting)?;
        match value {
            Value::Literal(value) => {
                convert(value, self.input.language_id, &call.name, kind).map(Value::Literal)
            }
            Value::Read(mut read) => {
                read.nullable = false;
                read.value_type = Some(kind.to_owned());
                if let Some(default) = read.default.take() {
                    read.default = convert(default, self.input.language_id, &call.name, kind);
                    if read.default.is_none() {
                        read.incomplete = Some("unproven_default_conversion".into());
                    }
                }
                Some(Value::Read(read))
            }
        }
    }

    /// Enclosing conversions describe this concrete read without creating a
    /// second contradictory raw-default usage for the same read occurrence.
    pub(super) fn effective_read<'a>(
        &self,
        mut node: Node<'a>,
        mut read: values::Read,
    ) -> (Node<'a>, values::Read) {
        let mut probe = node;
        for _ in 0..MAX_DEPTH {
            let Some(parent) = probe.parent() else {
                break;
            };
            if self.input.language_id == "rust"
                && parent.kind() == "field_expression"
                && parent.child_by_field_name("value") == Some(probe)
            {
                probe = parent;
                continue;
            }
            if matches!(
                parent.kind(),
                "argument_list" | "arguments" | "value_arguments" | "argument" | "value_argument"
            ) {
                probe = parent;
                continue;
            }
            if !syntax::is_read_candidate(parent.kind())
                && !matches!(
                    parent.kind(),
                    "parenthesized_expression"
                        | "binary_expression"
                        | "boolean_operator"
                        | "binary_operator"
                        | "elvis_expression"
                        | "infix_expression"
                        | "nil_coalescing_expression"
                        | "field_expression"
                        | "navigation_expression"
                )
            {
                break;
            }
            let Some(Value::Read(outer)) = self.evaluate(parent, 0, &mut BTreeSet::new()) else {
                if syntax::call(parent, self.input.content).is_some() {
                    read.incomplete = Some("unproven_enclosing_call".into());
                }
                break;
            };
            if outer.start != read.start || outer.end != read.end {
                break;
            }
            read = outer;
            node = parent;
            probe = parent;
        }
        (node, read)
    }
}

fn convert(
    value: values::Literal,
    language: &str,
    name: &str,
    kind: &str,
) -> Option<values::Literal> {
    let text = if kind == "boolean" {
        let result = if language == "csharp" {
            match value.text.trim().to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => return None,
            }
        } else {
            match value.kind.as_str() {
                "string" => !value.text.is_empty() && !(language == "php" && value.text == "0"),
                "boolean" => value.text == "true",
                "number" => value.text.parse::<f64>().ok()? != 0.0,
                _ => return None,
            }
        };
        result.to_string()
    } else {
        let raw = value.text.trim();
        if name == "int" || name.ends_with("Int32.Parse") || name == "int.Parse" {
            raw.parse::<i64>().ok()?.to_string()
        } else if raw.is_empty() && name == "Number" {
            "0".into()
        } else {
            let number = raw.parse::<f64>().ok()?;
            if !number.is_finite() {
                return None;
            }
            // PHP integer casts have truncation and width rules; do not invent them.
            if name == "intval" {
                return None;
            }
            number.to_string()
        }
    };
    Some(values::Literal {
        text,
        kind: kind.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn textual_false_obeys_each_languages_boolean_conversion() {
        for (language, name, expected) in [
            ("python", "bool", "true"),
            ("javascript", "Boolean", "true"),
            ("php", "boolval", "true"),
            ("csharp", "bool.Parse", "false"),
        ] {
            let value = convert(
                values::Literal {
                    text: "false".into(),
                    kind: "string".into(),
                },
                language,
                name,
                "boolean",
            )
            .unwrap();
            assert_eq!(value.text, expected);
        }
    }
}
