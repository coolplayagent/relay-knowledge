//! Bounded syntax-based configuration flow shared by non-Java code languages.
mod conversions;
mod imports;
mod properties;
mod scopes;
mod strings;
mod syntax;
mod values;

use super::*;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;
use values::{Atom, Value};

const MAX_NODES: usize = 262_144;
const MAX_DEPTH: usize = 16;
const MAX_EVALUATIONS: usize = 1_000_000;

struct Analysis<'a> {
    input: &'a FeatureFlagFileInput<'a>,
    nodes: Vec<Node<'a>>,
    imports: Vec<Node<'a>>,
    evaluations: Cell<usize>,
    declarations: BTreeMap<String, Vec<Node<'a>>>,
    functions: BTreeMap<String, Vec<Node<'a>>>,
    properties: BTreeMap<String, Vec<(Node<'a>, Node<'a>)>>,
    property_returns: BTreeMap<usize, (String, Node<'a>)>,
    module: String,
    side_effects: BTreeMap<String, Vec<(usize, usize)>>,
}

pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let Some(root) = input.syntax_root else {
        return Ok(Vec::new());
    };
    let mut analysis = Analysis {
        input,
        nodes: Vec::new(),
        imports: Vec::new(),
        evaluations: Cell::new(0),
        declarations: BTreeMap::new(),
        functions: BTreeMap::new(),
        properties: BTreeMap::new(),
        property_returns: BTreeMap::new(),
        module: syntax::module(input),
        side_effects: BTreeMap::new(),
    };
    let mut cursor = root.walk();
    loop {
        if analysis.nodes.len() == MAX_NODES {
            return Err(DomainError::invalid(
                "configuration",
                "syntax node budget exceeded",
            ));
        }
        let node = cursor.node();
        analysis.nodes.push(node);
        if matches!(
            node.kind(),
            "import"
                | "import_from_statement"
                | "import_statement"
                | "preproc_include"
                | "use_declaration"
                | "import_declaration"
                | "import_header"
                | "namespace_use_declaration"
                | "using_directive"
                | "require_expression"
                | "require_once_expression"
                | "include_expression"
                | "include_once_expression"
        ) || (matches!(input.language_id, "ruby" | "starlark")
            && node.kind() == "call"
            && syntax::call(node, input.content)
                .is_some_and(|c| matches!(c.name.as_str(), "load" | "require_relative")))
        {
            if analysis.imports.len() == 1024 {
                return Err(DomainError::invalid(
                    "configuration",
                    "import evidence budget exceeded",
                ));
            }
            analysis.imports.push(node);
        }
        if scopes::is_type(node.kind()) {
            if let Some(name) = node.child_by_field_name("name") {
                analysis
                    .declarations
                    .entry(syntax::text(name, input.content).to_owned())
                    .or_default()
                    .push(node);
            }
        }
        if let Some((name, _)) = syntax::assignment(node, input.content) {
            analysis.declarations.entry(name).or_default().push(node);
        }
        if matches!(node.kind(), "assignment" | "assignment_expression")
            && let Some(target) = node.child_by_field_name("left")
            && matches!(
                target.kind(),
                "attribute"
                    | "member_expression"
                    | "navigation_expression"
                    | "member_access_expression"
            )
        {
            for name in syntax::text(target, input.content)
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|s| !s.is_empty())
            {
                analysis
                    .side_effects
                    .entry(name.to_owned())
                    .or_default()
                    .push((node.start_byte(), node.end_byte()));
            }
        }
        if matches!(
            node.kind(),
            "global_statement" | "global_declaration" | "delete_statement"
        ) {
            let mut cursor = node.walk();
            for identifier in node.named_children(&mut cursor).take(1024) {
                if syntax::is_identifier(identifier.kind()) {
                    analysis
                        .side_effects
                        .entry(
                            syntax::text(identifier, input.content)
                                .trim_start_matches('$')
                                .to_owned(),
                        )
                        .or_default()
                        .push((node.start_byte(), node.end_byte()));
                }
            }
        }
        let call = syntax::call(node, input.content);
        if let Some(call) =
            call.filter(|c| syntax::reader_namespace(input.language_id, &c.name).is_none())
        {
            for arg in call.arguments {
                if !syntax::is_identifier(arg.kind())
                    && !matches!(
                        arg.kind(),
                        "pointer_expression"
                            | "reference_expression"
                            | "ref_expression"
                            | "argument"
                    )
                {
                    continue;
                }
                for name in syntax::text(arg, input.content)
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .filter(|s| !s.is_empty())
                {
                    analysis
                        .side_effects
                        .entry(name.to_owned())
                        .or_default()
                        .push((node.start_byte(), node.end_byte()));
                }
            }
        }
        if matches!(
            node.kind(),
            "update_expression"
                | "augmented_assignment"
                | "compound_assignment_expr"
                | "augmented_assignment_expression"
        ) {
            for name in syntax::text(node, input.content)
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|s| !s.is_empty())
            {
                analysis
                    .side_effects
                    .entry(name.to_owned())
                    .or_default()
                    .push((node.start_byte(), node.end_byte()));
            }
        }
        if let Some((name, value)) = properties::getter(node, input.language_id, input.content) {
            analysis
                .properties
                .entry(name.clone())
                .or_default()
                .push((node, value));
            analysis
                .property_returns
                .insert(value.start_byte(), (name, node));
        }
        if syntax::is_function(node.kind()) {
            if let Some(name) = syntax::function_name(node, input.content) {
                analysis.functions.entry(name).or_default().push(node);
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                for ranges in analysis.side_effects.values_mut() {
                    ranges.sort_unstable();
                    ranges.dedup();
                }
                return analysis.records();
            }
        }
    }
}

impl Analysis<'_> {
    fn records(&self) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
        let mut rows = Vec::new();
        for node in &self.nodes {
            if syntax::is_function(node.kind())
                && let Some(name) = syntax::function_name(*node, self.input.content)
            {
                let returns_config = syntax::zero_arguments(*node, self.input.content)
                    && self.stable_getter(*node, &name)
                    && syntax::single_return(*node, self.input.language_id)
                        .and_then(|value| self.evaluate(value, 0, &mut BTreeSet::new()))
                        .is_some_and(|value| matches!(value, Value::Read(_)));
                if !returns_config {
                    let binding = self.binding(*node, &name);
                    let mut row = record(
                        self.input,
                        "config_symbol",
                        &binding,
                        "declares_config_getter",
                        node.start_byte(),
                        node.end_byte(),
                    )?;
                    row.metadata.bindings = vec![binding.clone()];
                    row.metadata.declared_getter = Some(binding.clone());
                    row.metadata.reference = Some(format!(
                        "opaque:{binding}@{}:{}",
                        self.input.path,
                        node.start_byte()
                    ));
                    row.metadata.flow_incomplete = Some("unproven_getter_return".into());
                    check_fact_budget(rows.len())?;
                    rows.push(row);
                }
            }
            if syntax::is_read_candidate(node.kind()) {
                if let Some(Value::Read(read)) = self.evaluate(*node, 0, &mut BTreeSet::new()) {
                    if read.start != node.start_byte() || read.end != node.end_byte() {
                        continue;
                    }
                    let (expression, read) = self.effective_read(*node, read);
                    let mut row = self.read_record(*node, &read, "reads_config")?;
                    if let Some(function) = syntax::return_owner(expression, self.input.language_id)
                        && syntax::zero_arguments(function, self.input.content)
                        && let Some(name) = syntax::function_name(function, self.input.content)
                        && self.stable_getter(function, &name)
                    {
                        let binding = self.binding(function, &name);
                        row.metadata.bindings.push(binding.clone());
                        row.metadata.declared_getter = Some(binding);
                    }
                    if let Some((name, owner)) = self.property_returns.get(&expression.start_byte())
                        && self.stable_getter(*owner, name)
                    {
                        let binding = self.binding(*owner, name);
                        row.metadata.bindings = vec![binding.clone()];
                        row.metadata.declared_getter = Some(binding);
                    }
                    check_fact_budget(rows.len())?;
                    rows.push(row);
                }
            }
            if let Some(condition) = syntax::condition(*node) {
                let mut seen = BTreeSet::new();
                self.condition_reads(condition, &mut seen, &mut rows)?;
            }
            if let Some((name, value)) = syntax::assignment(*node, self.input.content) {
                if self.stable_constant(*node, &name)
                    && let Some(Value::Literal(value)) =
                        self.evaluate(value, 0, &mut BTreeSet::new())
                    && value.kind == "string"
                {
                    let mut row = record(
                        self.input,
                        "config_key",
                        &value.text,
                        "declares_string_constant",
                        node.start_byte(),
                        node.end_byte(),
                    )?;
                    row.metadata.bindings.push(self.binding(*node, &name));
                    check_fact_budget(rows.len())?;
                    rows.push(row);
                }
            }
        }
        if self.evaluations.get() >= MAX_EVALUATIONS {
            return Err(DomainError::invalid(
                "configuration",
                "expression work budget exceeded",
            ));
        }
        Ok(rows)
    }

    fn binding(&self, node: Node<'_>, name: &str) -> String {
        let (public, default_export) =
            scopes::export_status(node, self.input.language_id, self.input.content);
        let prefix = if public { "" } else { "local:" };
        let name = if default_export { "default" } else { name };
        format!(
            "{prefix}{}|{}|{name}",
            self.module,
            scopes::lexical_identity(node, self.input.content)
        )
    }

    fn read_record(
        &self,
        node: Node<'_>,
        read: &values::Read,
        edge: &str,
    ) -> Result<CodeFeatureFlagRecord, DomainError> {
        let (kind, key, reference) = match &read.key {
            Atom::Literal(key) => (read.namespace.as_str(), key.as_str(), None),
            Atom::Reference(key) => ("config_symbol", key.as_str(), Some(key.clone())),
        };
        let mut row = record(
            self.input,
            kind,
            key,
            edge,
            node.start_byte(),
            node.end_byte(),
        )?;
        row.metadata.reference = reference;
        row.metadata.exact_reference =
            row.metadata.reference.is_some() && read.namespace.is_empty();
        if row.metadata.reference.is_some() && !read.namespace.is_empty() {
            row.metadata.target_kind = Some(read.namespace.clone());
        }
        row.metadata.value_type.clone_from(&read.value_type);
        if let Some(default) = &read.default {
            row.metadata.default_value = Some(default.text.clone());
            row.metadata.value_type = Some(default.kind.clone());
        }
        row.metadata.flow_incomplete.clone_from(&read.incomplete);
        if edge == "guards_code" {
            let original = record(self.input, kind, key, "reads_config", read.start, read.end)?;
            row.metadata.read_usage_id = Some(original.usage_id);
        }
        Ok(row)
    }

    fn condition_reads(
        &self,
        root: Node<'_>,
        seen: &mut BTreeSet<(usize, usize)>,
        rows: &mut Vec<CodeFeatureFlagRecord>,
    ) -> Result<(), DomainError> {
        let mut cursor = root.walk();
        let mut count = 0;
        loop {
            count += 1;
            if count > 8192 {
                return Err(DomainError::invalid(
                    "configuration",
                    "condition syntax budget exceeded",
                ));
            }
            let node = cursor.node();
            let evaluated = self.evaluate(node, 0, &mut BTreeSet::new());
            let known_read = matches!(evaluated, Some(Value::Read(_)));
            if let Some(Value::Read(read)) = evaluated {
                if seen.insert((read.start, read.end)) {
                    check_fact_budget(rows.len())?;
                    rows.push(self.read_record(root, &read, "guards_code")?);
                }
            }
            let opaque = syntax::call(node, self.input.content).is_some_and(|c| {
                !known_read && !c.name.ends_with(".is_ok") && !c.name.ends_with(".is_some")
            });
            if !opaque && !syntax::is_function(node.kind()) && cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod mod_tests;
