//! Bounded lexical names for configuration constants and getter receivers.
use tree_sitter::Node;
pub(super) fn text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    &content[node.byte_range()]
}
fn root(mut node: Node<'_>) -> Node<'_> {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}
fn is_type(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration"
    )
}

pub(super) fn qualified(node: Node<'_>, name: &str, content: &str) -> String {
    let mut depth = 0usize;
    let erased = name
        .chars()
        .filter(|ch| match ch {
            '<' => {
                depth += 1;
                false
            }
            '>' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0 && !ch.is_whitespace(),
        })
        .collect::<String>();
    let name = erased.as_str();
    let (head, suffix) = name
        .split_once('.')
        .map_or((name, ""), |(head, _)| (head, &name[head.len()..]));
    if let Some(owner) = super::types::lexical(node, head, content) {
        return format!("{owner}{suffix}");
    }
    let mut local_type = false;
    let mut wildcards = std::collections::BTreeSet::new();
    let mut package = String::new();
    let mut cursor = root(node).walk();
    for child in root(node).named_children(&mut cursor) {
        let source = text(child, content);
        if is_type(child)
            && child
                .child_by_field_name("name")
                .is_some_and(|n| text(n, content) == head)
        {
            local_type = true;
        }
        if child.kind() == "package_declaration" {
            package = source
                .trim_start_matches("package")
                .trim()
                .trim_end_matches(';')
                .trim()
                .to_owned();
        }
        if child.kind() == "import_declaration" && !source.contains("static ") {
            let imported = source
                .trim_start_matches("import")
                .trim()
                .trim_end_matches(';')
                .trim();
            if let Some(prefix) = imported.strip_suffix(".*") {
                wildcards.insert(prefix.to_owned());
            }
            if imported.rsplit('.').next() == Some(head) {
                return format!("{imported}{suffix}");
            }
        }
    }
    if !local_type
        && (suffix.is_empty() || head.chars().next().is_some_and(char::is_uppercase))
        && !wildcards.is_empty()
    {
        return format!("<ambiguous-import>.{name}");
    }
    // A dotted name beginning with a type is a relative nested type. Package
    // prefixes remain qualified; imports and local declarations take priority.
    if package.is_empty()
        || (!suffix.is_empty()
            && !local_type
            && !head.chars().next().is_some_and(char::is_uppercase))
        || name.starts_with(&format!("{package}."))
    {
        name.to_owned()
    } else {
        format!("{package}.{name}")
    }
}
pub(super) fn field_symbol(mut node: Node<'_>, name: &str, content: &str) -> String {
    let mut owners = Vec::new();
    while let Some(parent) = node.parent() {
        if is_type(parent) {
            if let Some(name) = parent.child_by_field_name("name") {
                owners.push(text(name, content));
            }
        }
        node = parent;
    }
    owners.reverse();
    owners.push(name);
    let suffix = owners.join(".");
    let head = owners.first().copied().unwrap_or(name);
    let qualified_head = qualified(node, head, content);
    format!("{}{}", qualified_head, &suffix[head.len()..])
}
pub(super) fn key_literal(node: Node<'_>, content: &str) -> Option<String> {
    if matches!(node.kind(), "identifier" | "field_access") {
        return None;
    }
    string_expression(node, content, 0)
        .then(|| literal(node, content, 0))
        .flatten()
}
fn string_expression(node: Node<'_>, content: &str, depth: usize) -> bool {
    if depth >= 16 {
        return false;
    }
    match node.kind() {
        "string_literal" => true,
        "parenthesized_expression" => node
            .named_child(0)
            .is_some_and(|n| string_expression(n, content, depth + 1)),
        "binary_expression" => {
            node.child_by_field_name("operator")
                .is_some_and(|n| text(n, content) == "+")
                && ["left", "right"].iter().any(|field| {
                    node.child_by_field_name(field)
                        .is_some_and(|n| string_expression(n, content, depth + 1))
                })
        }
        "identifier" | "field_access" => constant_binding(node, content)
            .and_then(|n| n.parent())
            .and_then(|n| n.child_by_field_name("type"))
            .is_some_and(|n| matches!(text(n, content), "String" | "java.lang.String")),
        _ => false,
    }
}
pub(super) fn literal(node: Node<'_>, content: &str, depth: usize) -> Option<String> {
    let mut budget = 256;
    literal_bounded(node, content, depth, &mut budget)
}
fn literal_bounded(
    node: Node<'_>,
    content: &str,
    depth: usize,
    budget: &mut usize,
) -> Option<String> {
    *budget = budget.checked_sub(1)?;
    if depth >= 16 {
        return None;
    }
    let value = match node.kind() {
        "string_literal" => {
            let raw = text(node, content).strip_prefix('"')?.strip_suffix('"')?;
            super::strings::decode(raw)
        }
        "parenthesized_expression" => {
            literal_bounded(node.named_child(0)?, content, depth + 1, budget)
        }
        "binary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|op| text(op, content) == "+") =>
        {
            Some(format!(
                "{}{}",
                literal_bounded(
                    node.child_by_field_name("left")?,
                    content,
                    depth + 1,
                    budget
                )?,
                literal_bounded(
                    node.child_by_field_name("right")?,
                    content,
                    depth + 1,
                    budget
                )?
            ))
        }
        "true" | "false" | "decimal_integer_literal" | "decimal_floating_point_literal" => {
            Some(text(node, content).to_owned())
        }
        "identifier" | "field_access" if depth > 0 => {
            let declaration = constant_binding(node, content)?;
            let owner = declaration.parent()?;
            let mut cursor = owner.walk();
            let is_final = owner.kind() == "constant_declaration"
                || owner.named_children(&mut cursor).any(|n| {
                    n.kind() == "modifiers"
                        && text(n, content)
                            .split_whitespace()
                            .any(|word| word == "final")
                });
            if !is_final
                || !owner
                    .child_by_field_name("type")
                    .is_some_and(|ty| matches!(text(ty, content), "String" | "java.lang.String"))
            {
                return None;
            }
            literal_bounded(
                declaration.child_by_field_name("value")?,
                content,
                depth + 1,
                budget,
            )
        }
        _ => None,
    }?;
    (value.len() <= 65_536).then_some(value)
}
fn constant_binding<'a>(node: Node<'a>, content: &str) -> Option<Node<'a>> {
    if node.kind() == "identifier" {
        return binding(node, text(node, content), content);
    }
    let symbol = key_symbol(node, content)?;
    let mut cursor = root(node).walk();
    for _ in 0..4096 {
        let candidate = cursor.node();
        if candidate.kind() == "variable_declarator" {
            if let Some(name) = candidate.child_by_field_name("name") {
                if field_symbol(candidate, text(name, content), content) == symbol {
                    return Some(candidate);
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
    None
}
pub(super) fn binding<'a>(mut node: Node<'a>, name: &str, content: &str) -> Option<Node<'a>> {
    let explicit_field = node.parent().is_some_and(|parent| {
        parent.kind() == "field_access"
            && parent.child_by_field_name("field") == Some(node)
            && parent
                .child_by_field_name("object")
                .is_some_and(|object| object.kind() == "this")
    });
    let position = node.start_byte();
    let mut budget = 2048;
    while let Some(parent) = node.parent() {
        if budget == 0 {
            return None;
        }
        budget -= 1;
        if !explicit_field
            && parent.kind() == "enhanced_for_statement"
            && parent.child_by_field_name("body") == Some(node)
            && parent
                .child_by_field_name("name")
                .is_some_and(|n| text(n, content) == name)
        {
            return Some(parent);
        }
        if let Some(parameters) = parent
            .child_by_field_name("parameters")
            .filter(|_| !explicit_field)
        {
            if parameters.kind() == "identifier" && text(parameters, content) == name {
                return Some(parameters);
            }
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                if parameter.kind() == "identifier" && text(parameter, content) == name {
                    return Some(parameter);
                }
                if parameter
                    .child_by_field_name("name")
                    .is_some_and(|n| text(n, content) == name)
                {
                    return Some(parameter);
                }
            }
        }
        let condition = match parent.kind() {
            "if_statement" | "ternary_expression"
                if parent.child_by_field_name("consequence") == Some(node) =>
            {
                parent.child_by_field_name("condition").map(|n| (n, true))
            }
            "if_statement" | "ternary_expression"
                if parent.child_by_field_name("alternative") == Some(node) =>
            {
                parent.child_by_field_name("condition").map(|n| (n, false))
            }
            "while_statement" | "for_statement"
                if parent.child_by_field_name("body") == Some(node) =>
            {
                parent.child_by_field_name("condition").map(|n| (n, true))
            }
            "binary_expression" if parent.child_by_field_name("right") == Some(node) => parent
                .child_by_field_name("left")
                .zip(parent.child_by_field_name("operator").and_then(|op| {
                    match text(op, content) {
                        "&&" => Some(true),
                        "||" => Some(false),
                        _ => None,
                    }
                })),
            _ => None,
        };
        if !explicit_field {
            if let Some((condition, truth)) = condition {
                if let Some(pattern) = pattern_binding(condition, truth, name, content, &mut budget)
                {
                    return Some(pattern);
                }
                if budget == 0 {
                    return None;
                }
            }
        }
        if matches!(parent.kind(), "block" | "class_body" | "for_statement")
            && (!explicit_field || parent.kind() == "class_body")
        {
            let mut found = None;
            let mut cursor = parent.walk();
            for declaration in parent.named_children(&mut cursor) {
                if budget == 0 {
                    return None;
                }
                budget -= 1;
                if parent.kind() != "class_body" && declaration.start_byte() >= position {
                    break;
                }
                if !explicit_field && declaration.kind() == "if_statement" {
                    let exits = declaration
                        .child_by_field_name("consequence")
                        .is_some_and(abrupt_exit);
                    let else_exits = declaration
                        .child_by_field_name("alternative")
                        .is_some_and(abrupt_exit);
                    if exits || else_exits {
                        if let Some(condition) = declaration.child_by_field_name("condition") {
                            if let Some(pattern) =
                                pattern_binding(condition, !exits, name, content, &mut budget)
                            {
                                found = Some(pattern);
                            }
                            if budget == 0 {
                                return None;
                            }
                        }
                    }
                }
                if matches!(
                    declaration.kind(),
                    "local_variable_declaration" | "field_declaration" | "constant_declaration"
                ) {
                    let mut names = declaration.walk();
                    for variable in declaration.named_children(&mut names) {
                        if variable.kind() == "variable_declarator"
                            && variable
                                .child_by_field_name("name")
                                .is_some_and(|n| text(n, content) == name)
                        {
                            found = Some(variable);
                        }
                    }
                }
            }
            if found.is_some() {
                return found;
            }
        }
        if parent.kind() == "object_creation_expression" {
            return None;
        }
        node = parent;
    }
    None
}
pub(super) fn receiver_type(node: Node<'_>, content: &str, depth: usize) -> Option<String> {
    if depth >= 16 {
        return None;
    }
    match node.kind() {
        "this" => Some(
            field_symbol(node, "", content)
                .trim_end_matches('.')
                .to_owned(),
        ),
        "identifier" => {
            let Some(declaration) = binding(node, text(node, content), content) else {
                return super::types::static_receiver(node, content);
            };
            let owner = if declaration.kind() == "variable_declarator" {
                declaration.parent()?
            } else {
                declaration
            };
            let ty = owner
                .child_by_field_name("type")
                .or_else(|| owner.child_by_field_name("right"))?;
            if declaration.child_by_field_name("dimensions").is_some() {
                return None;
            }
            if text(ty, content) == "var" {
                return receiver_type(
                    declaration.child_by_field_name("value")?,
                    content,
                    depth + 1,
                );
            }
            if declaration
                .child_by_field_name("value")
                .is_some_and(|value| anonymous(value))
            {
                return None;
            }
            Some(qualified(node, text(ty, content), content))
        }
        "parenthesized_expression" => receiver_type(node.named_child(0)?, content, depth + 1),
        "cast_expression" => {
            if anonymous(node.child_by_field_name("value")?) {
                return None;
            }
            Some(qualified(
                node,
                text(node.child_by_field_name("type")?, content),
                content,
            ))
        }
        "object_creation_expression" if !anonymous(node) => Some(qualified(
            node,
            text(node.child_by_field_name("type")?, content),
            content,
        )),
        "field_access"
            if node
                .child_by_field_name("object")
                .is_some_and(|n| n.kind() == "this") =>
        {
            receiver_type(node.child_by_field_name("field")?, content, depth + 1)
        }
        "field_access" => super::types::static_receiver(node, content),
        _ => None,
    }
}
fn anonymous(mut node: Node<'_>) -> bool {
    for _ in 0..16 {
        match node.kind() {
            "parenthesized_expression" => {
                let Some(child) = node.named_child(0) else {
                    return true;
                };
                node = child;
            }
            "cast_expression" => {
                let Some(child) = node.child_by_field_name("value") else {
                    return true;
                };
                node = child;
            }
            _ => {
                let mut cursor = node.walk();
                return node
                    .named_children(&mut cursor)
                    .any(|child| child.kind() == "class_body");
            }
        }
    }
    true
}

pub(super) fn key_symbol(node: Node<'_>, content: &str) -> Option<String> {
    match node.kind() {
        "identifier" => {
            let name = text(node, content);
            if let Some(declaration) = binding(node, name, content) {
                if declaration.parent().is_some_and(|p| {
                    matches!(p.kind(), "field_declaration" | "constant_declaration")
                }) {
                    return Some(field_symbol(declaration, name, content));
                }
                return None;
            }
            let mut cursor = root(node).walk();
            for import in root(node)
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "import_declaration")
            {
                if let Some(path) = text(import, content)
                    .trim()
                    .strip_prefix("import static ")
                    .map(|s| s.trim_end_matches(';').trim())
                {
                    if path.rsplit('.').next() == Some(name) {
                        return Some(path.to_owned());
                    }
                }
            }
            None
        }
        "field_access" => {
            let object = node.child_by_field_name("object")?;
            let field = node.child_by_field_name("field")?;
            let owner = text(object, content);
            if object.kind() == "this" {
                let name = text(field, content);
                let declaration = binding(field, name, content)?;
                return Some(field_symbol(declaration, name, content));
            }
            if object.kind() == "identifier" && binding(object, owner, content).is_some() {
                return None;
            }
            Some(format!(
                "{}.{}",
                qualified(node, owner, content),
                text(field, content)
            ))
        }
        _ => None,
    }
}
pub(super) fn platform_visible(node: Node<'_>, name: &str, content: &str) -> bool {
    if binding(node, name, content).is_some() {
        return false;
    }
    let position = node.start_byte();
    let mut scope = Some(node);
    let mut budget = 4096usize;
    while let Some(current) = scope {
        if is_type(current)
            && current
                .child_by_field_name("name")
                .is_some_and(|n| text(n, content) == name)
        {
            return false;
        }
        if matches!(current.kind(), "program" | "class_body" | "block") {
            let mut cursor = current.walk();
            for candidate in current.named_children(&mut cursor) {
                let Some(remaining) = budget.checked_sub(1) else {
                    return false;
                };
                budget = remaining;
                if is_type(candidate)
                    && (current.kind() != "block" || candidate.start_byte() < position)
                    && candidate
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, content) == name)
                {
                    return false;
                }
                if candidate.kind() == "import_declaration" {
                    let imported = text(candidate, content)
                        .trim_start_matches("import")
                        .trim()
                        .trim_end_matches(';')
                        .trim();
                    if imported.rsplit('.').next() == Some(name)
                        && imported != format!("java.lang.{name}")
                    {
                        return false;
                    }
                }
            }
        }
        scope = current.parent();
    }
    true
}
/// Static platform imports must name the real Java owner and have no local method shadow.
pub(super) fn static_owner(node: Node<'_>, method: &str, content: &str) -> Option<&'static str> {
    let mut single = std::collections::BTreeSet::new();
    let mut wildcard = std::collections::BTreeSet::new();
    let mut pending = vec![root(node)];
    let mut budget = 4096;
    while let Some(current) = pending.pop() {
        if budget == 0 {
            return None;
        }
        budget -= 1;
        if current.kind() == "method_declaration"
            && current
                .child_by_field_name("name")
                .is_some_and(|n| text(n, content) == method)
            && super::static_imports::shadows(current, node, content)
        {
            return None;
        }
        if current.kind() == "import_declaration" {
            if let Some(path) = text(current, content)
                .trim()
                .strip_prefix("import static ")
                .map(|v| v.trim_end_matches(';').trim())
            {
                if let Some((owner, member)) = path.rsplit_once('.') {
                    if member == method {
                        single.insert(owner.to_owned());
                    }
                    if member == "*" {
                        wildcard.insert(owner.to_owned());
                    }
                }
            }
        }
        if current.kind() == "program" || is_type(current) || current.kind() == "class_body" {
            let mut cursor = current.walk();
            pending.extend(current.named_children(&mut cursor));
        }
    }
    let candidates = if single.is_empty() { wildcard } else { single };
    if candidates.len() != 1 {
        return None;
    }
    match candidates.first()?.as_str() {
        "java.lang.System" => Some("java.lang.System"),
        "java.lang.Boolean" => Some("java.lang.Boolean"),
        "java.lang.Integer" => Some("java.lang.Integer"),
        "java.lang.Long" => Some("java.lang.Long"),
        "java.lang.Double" => Some("java.lang.Double"),
        _ => None,
    }
}

fn pattern_binding<'a>(
    condition: Node<'a>,
    truth: bool,
    name: &str,
    content: &str,
    budget: &mut usize,
) -> Option<Node<'a>> {
    let mut pending = vec![(condition, truth)];
    while let Some((node, truth)) = pending.pop() {
        *budget = budget.checked_sub(1)?;
        match node.kind() {
            "instanceof_expression"
                if truth
                    && node
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, content) == name) =>
            {
                return Some(node);
            }
            "parenthesized_expression" => pending.push((node.named_child(0)?, truth)),
            "unary_expression"
                if node
                    .child_by_field_name("operator")
                    .is_some_and(|op| text(op, content) == "!") =>
            {
                pending.push((node.child_by_field_name("operand")?, !truth))
            }
            "binary_expression" => {
                let operator = text(node.child_by_field_name("operator")?, content);
                if (truth && operator == "&&") || (!truth && operator == "||") {
                    pending.push((node.child_by_field_name("left")?, truth));
                    pending.push((node.child_by_field_name("right")?, truth));
                }
            }
            _ => {}
        }
    }
    None
}
fn abrupt_exit(mut node: Node<'_>) -> bool {
    for _ in 0..16 {
        if matches!(node.kind(), "return_statement" | "throw_statement") {
            return true;
        }
        if node.kind() != "block" {
            return false;
        }
        let Some(last) = node
            .named_child_count()
            .checked_sub(1)
            .and_then(|i| node.named_child(i as u32))
        else {
            return false;
        };
        node = last;
    }
    false
}
