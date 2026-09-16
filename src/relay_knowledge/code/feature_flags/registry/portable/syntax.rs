//! Grammar adapters for configuration expressions and lexical boundaries.
use super::*;

pub(super) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source
        .get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
        .trim()
}

pub(super) fn is_identifier(kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "simple_identifier" | "variable_name" | "constant" | "word" | "name"
    )
}

pub(super) fn is_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "function_item"
            | "method_declaration"
            | "method_definition"
            | "method"
            | "singleton_method"
    )
}

pub(super) fn is_read_candidate(kind: &str) -> bool {
    matches!(
        kind,
        "call"
            | "call_expression"
            | "invocation_expression"
            | "method_invocation"
            | "function_call_expression"
            | "member_call_expression"
            | "scoped_call_expression"
            | "member_expression"
            | "subscript_expression"
            | "element_access_expression"
            | "element_reference"
            | "attribute"
            | "subscript"
    )
}

pub(super) fn assignment<'a>(node: Node<'a>, source: &str) -> Option<(String, Node<'a>)> {
    if !matches!(
        node.kind(),
        "assignment"
            | "assignment_expression"
            | "variable_declarator"
            | "const_item"
            | "let_declaration"
            | "const_spec"
            | "var_spec"
            | "short_var_declaration"
            | "property_declaration"
            | "val_definition"
            | "var_definition"
            | "init_declarator"
            | "variable_assignment"
            | "const_element"
    ) {
        return None;
    }
    let mut name = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("left"))
        .or_else(|| node.child_by_field_name("pattern"))
        .or_else(|| node.child_by_field_name("declarator"))
        .or_else(|| node.named_child(0))?;
    let value = node
        .child_by_field_name("value")
        .or_else(|| node.child_by_field_name("right"))
        .or_else(|| {
            node.named_child(u32::try_from(node.named_child_count().checked_sub(1)?).ok()?)
        })?;
    for _ in 0..8 {
        if is_identifier(name.kind()) {
            break;
        }
        let next = name
            .child_by_field_name("declarator")
            .or_else(|| name.child_by_field_name("bound_identifier"))
            .or_else(|| {
                (name.named_child_count() == 1)
                    .then(|| name.named_child(0))
                    .flatten()
            });
        let Some(next) = next else {
            break;
        };
        name = next;
    }
    let name_text = text(name, source).trim_start_matches('$');
    if name_text.is_empty()
        || !name_text.chars().all(|c| c.is_alphanumeric() || c == '_')
        || name.id() == value.id()
    {
        return None;
    }
    let value = if matches!(value.kind(), "expression_list") && value.named_child_count() == 1 {
        value.named_child(0)?
    } else {
        value
    };
    Some((name_text.to_owned(), value))
}

pub(super) fn function_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut name = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("declarator"))?;
    for _ in 0..8 {
        if is_identifier(name.kind())
            || matches!(name.kind(), "property_identifier" | "field_identifier")
        {
            return Some(text(name, source).to_owned());
        }
        name = name
            .child_by_field_name("declarator")
            .or_else(|| name.child_by_field_name("name"))?;
    }
    None
}

pub(super) fn condition(node: Node<'_>) -> Option<Node<'_>> {
    if !matches!(
        node.kind(),
        "if_statement"
            | "if_expression"
            | "if"
            | "unless"
            | "while_statement"
            | "while_expression"
            | "while"
            | "conditional_expression"
            | "elif_clause"
    ) {
        return None;
    }
    node.child_by_field_name("condition")
        .or_else(|| node.child_by_field_name("test"))
}

pub(super) fn scope(mut node: Node<'_>) -> Node<'_> {
    for _ in 0..128 {
        let Some(parent) = node.parent() else {
            return node;
        };
        node = parent;
        if is_function(node.kind())
            || matches!(
                node.kind(),
                "class_definition"
                    | "class_declaration"
                    | "class"
                    | "block"
                    | "statement_block"
                    | "compound_statement"
                    | "body_statement"
                    | "mod_item"
            )
        {
            return node;
        }
    }
    node
}

pub(super) fn visible(declaration: Node<'_>, use_site: Node<'_>) -> bool {
    if declaration.start_byte() >= use_site.start_byte() {
        return false;
    }
    let boundary = scope(declaration);
    // A nested function can execute after mutations; only its own lexical bindings are followed.
    let mut current = Some(use_site);
    for _ in 0..128 {
        let Some(node) = current else {
            return false;
        };
        if node.id() == boundary.id() {
            return true;
        }
        current = node.parent();
    }
    false
}

pub(super) fn visible_function(
    function: Node<'_>,
    use_site: Node<'_>,
    call: &str,
    _source: &str,
) -> bool {
    if call.contains('.') && !call.starts_with("self.") && !call.starts_with("this.") {
        return false;
    }
    let boundary = scope(function);
    let mut current = Some(use_site);
    for _ in 0..128 {
        let Some(node) = current else {
            return false;
        };
        if node.id() == boundary.id() {
            return true;
        }
        current = node.parent();
    }
    false
}

pub(super) fn constant_declaration(node: Node<'_>, language: &str, source: &str) -> bool {
    let Some((name, _)) = assignment(node, source) else {
        return false;
    };
    if matches!(language, "python" | "ruby" | "starlark") {
        return name.chars().all(|c| !c.is_lowercase());
    }
    if matches!(
        node.kind(),
        "const_item" | "const_spec" | "val_definition" | "const_element"
    ) {
        return true;
    }
    let mut statement = node;
    for _ in 0..4 {
        let head = text(statement, source);
        let words: Vec<_> = head.split_whitespace().take(8).collect();
        if words.iter().any(|word| {
            *word == "const"
                || (*word == "val" && matches!(language, "kotlin" | "scala"))
                || (*word == "let" && matches!(language, "rust" | "swift"))
        }) {
            return true;
        }
        if syntax::is_function(statement.kind()) || scopes::is_type(statement.kind()) {
            break;
        }
        let Some(parent) = statement.parent() else {
            break;
        };
        if matches!(
            parent.kind(),
            "source_file"
                | "program"
                | "module"
                | "translation_unit"
                | "block"
                | "compound_statement"
                | "statement_block"
        ) {
            break;
        }
        statement = parent;
    }
    false
}

pub(super) fn zero_arguments(function: Node<'_>, source: &str) -> bool {
    let mut declarator = function;
    for _ in 0..8 {
        if let Some(parameters) = declarator.child_by_field_name("parameters") {
            let body = text(parameters, source).trim_matches(['(', ')']).trim();
            return matches!(body, "" | "self" | "&self" | "&mut self" | "void");
        }
        let Some(next) = declarator.child_by_field_name("declarator") else {
            break;
        };
        declarator = next;
    }
    let mut cursor = function.walk();
    if let Some(parameters) = function
        .named_children(&mut cursor)
        .find(|n| n.kind() == "function_value_parameters")
    {
        return parameters.named_child_count() == 0;
    }
    if matches!(function.kind(), "method" | "singleton_method") {
        return true;
    }
    // Swift represents an empty parameter clause using unnamed delimiters.
    let mut cursor = function.walk();
    let children: Vec<_> = function.children(&mut cursor).take(256).collect();
    children
        .windows(2)
        .any(|pair| pair[0].kind() == "(" && pair[1].kind() == ")")
}

pub(super) fn single_return<'a>(function: Node<'a>, language: &str) -> Option<Node<'a>> {
    let mut body = function.child_by_field_name("body").or_else(|| {
        let mut cursor = function.walk();
        function
            .named_children(&mut cursor)
            .find(|n| n.kind() == "function_body")
    })?;
    let mut explicit = false;
    for _ in 0..16 {
        if matches!(
            body.kind(),
            "return_statement" | "return_expression" | "control_transfer_statement"
        ) {
            explicit = true;
        }
        if is_read_candidate(body.kind())
            || is_identifier(body.kind())
            || matches!(body.kind(), "binary_expression" | "binary_operator")
        {
            return (explicit
                || matches!(language, "rust" | "ruby" | "scala")
                || function.kind() == "arrow_function")
                .then_some(body);
        }
        if !matches!(
            body.kind(),
            "block"
                | "statement_block"
                | "compound_statement"
                | "function_body"
                | "statement_list"
                | "statements"
                | "body_statement"
                | "expression_list"
                | "expression_statement"
                | "return_statement"
                | "return_expression"
                | "control_transfer_statement"
        ) {
            return None;
        }
        let mut cursor = body.walk();
        let mut children = body.named_children(&mut cursor).filter(|n| !n.is_extra());
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        body = child;
    }
    None
}

pub(super) fn return_owner<'a>(mut node: Node<'a>, language: &str) -> Option<Node<'a>> {
    let original = node;
    for _ in 0..128 {
        node = node.parent()?;
        if is_function(node.kind()) {
            return (single_return(node, language) == Some(original)).then_some(node);
        }
    }
    None
}

pub(super) struct Call<'a> {
    pub name: String,
    pub arguments: Vec<Node<'a>>,
    pub literal_key: Option<String>,
}
pub(super) fn call<'a>(node: Node<'a>, source: &str) -> Option<Call<'a>> {
    if !is_read_candidate(node.kind()) {
        return None;
    }
    let mut assignment_target = node;
    for _ in 0..MAX_DEPTH {
        match assignment_target.parent() {
            Some(parent)
                if matches!(
                    parent.kind(),
                    "left_assignment_list"
                        | "pattern_list"
                        | "tuple_pattern"
                        | "parenthesized_expression"
                ) =>
            {
                assignment_target = parent
            }
            _ => break,
        }
    }
    if assignment_target.parent().is_some_and(|parent| {
        matches!(parent.kind(), "assignment" | "assignment_expression")
            && parent.child_by_field_name("left") == Some(assignment_target)
            && {
                let mut cursor = parent.walk();
                parent
                    .children(&mut cursor)
                    .take(16)
                    .any(|child| child.kind() == "=")
            }
    }) {
        return None;
    }
    if matches!(node.kind(), "member_expression" | "attribute") {
        // A method selector is syntax belonging to a call, not a configuration
        // property read (for example os.environ.get or process.env.hasOwnProperty).
        let mut selector = node;
        for _ in 0..MAX_DEPTH {
            match selector.parent() {
                Some(parent)
                    if parent.kind() == "parenthesized_expression"
                        && parent.named_child_count() == 1 =>
                {
                    selector = parent
                }
                _ => break,
            }
        }
        if selector.parent().is_some_and(|parent| {
            parent.child_by_field_name("function") == Some(selector)
                || parent.child_by_field_name("method") == Some(selector)
        }) {
            return None;
        }
        let object = node.child_by_field_name("object")?;
        if node.kind() == "attribute" && text(object, source) == "os.environ" {
            return None;
        }
        let property = node
            .child_by_field_name("property")
            .or_else(|| node.child_by_field_name("attribute"))?;
        return Some(Call {
            name: text(object, source).to_owned(),
            arguments: vec![],
            literal_key: Some(text(property, source).to_owned()),
        });
    }
    if matches!(
        node.kind(),
        "subscript" | "subscript_expression" | "element_access_expression" | "element_reference"
    ) {
        let object = node
            .child_by_field_name("value")
            .or_else(|| node.child_by_field_name("object"))
            .or_else(|| node.named_child(0))?;
        let key = node
            .child_by_field_name("subscript")
            .or_else(|| node.child_by_field_name("index"))
            .or_else(|| node.named_child(1))?;
        let key = if key.kind() == "argument_list" && key.named_child_count() == 1 {
            key.named_child(0)?
        } else {
            key
        };
        return Some(Call {
            name: text(object, source).replace("::", "."),
            arguments: vec![key],
            literal_key: None,
        });
    }
    let mut function = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("method"))
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.named_child(0))?;
    for _ in 0..MAX_DEPTH {
        if function.kind() != "parenthesized_expression" || function.named_child_count() != 1 {
            break;
        }
        function = function.named_child(0)?;
    }
    let mut name = text(function, source).replace("::", ".").replace("->", ".");
    if let Some(receiver) = node
        .child_by_field_name("receiver")
        .or_else(|| node.child_by_field_name("object"))
    {
        if receiver != function {
            name = format!("{}.{}", text(receiver, source), name);
        }
    }
    let arguments = node.child_by_field_name("arguments").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|n| matches!(n.kind(), "argument_list" | "arguments" | "value_arguments"))
    });
    let arguments = arguments.or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|n| n.kind() == "call_suffix")
            .and_then(|suffix| {
                let mut cursor = suffix.walk();
                suffix
                    .named_children(&mut cursor)
                    .find(|n| n.kind() == "value_arguments")
            })
    });
    let mut output = Vec::new();
    if let Some(arguments) = arguments {
        if arguments.named_child_count() > 4 {
            return None;
        }
        let mut cursor = arguments.walk();
        for arg in arguments.named_children(&mut cursor).take(4) {
            let arg = if matches!(arg.kind(), "argument" | "value_argument")
                && arg.named_child_count() == 1
            {
                arg.named_child(0)?
            } else {
                arg
            };
            output.push(arg);
        }
    }
    Some(Call {
        name,
        arguments: output,
        literal_key: None,
    })
}

pub(super) fn reader_namespace(language: &str, name: &str) -> Option<&'static str> {
    let environment = match language {
        "python" => matches!(name, "os.getenv" | "os.environ.get" | "os.environ"),
        "javascript" | "jsx" | "typescript" | "tsx" => matches!(
            name,
            "process.env" | "Deno.env.get" | "Bun.env" | "import.meta.env"
        ),
        "rust" => matches!(
            name,
            "std.env.var" | "std.env.var_os" | "env.var" | "env.var_os"
        ),
        "c" | "cpp" => matches!(name, "getenv" | "std.getenv"),
        "go" => matches!(name, "os.Getenv" | "os.LookupEnv"),
        "csharp" => matches!(
            name,
            "Environment.GetEnvironmentVariable" | "System.Environment.GetEnvironmentVariable"
        ),
        "kotlin" | "scala" => matches!(name, "System.getenv" | "sys.env.get" | "sys.env.getOrElse"),
        "ruby" => matches!(name, "ENV" | "ENV.fetch"),
        "php" => matches!(name, "getenv" | "$_ENV" | "$_SERVER"),
        "swift" => name == "ProcessInfo.processInfo.environment",
        // Starlark has no receiver type annotations. Preserve a getenv
        // candidate, with receiver uncertainty recorded by the evaluator.
        "starlark" => name
            .rsplit_once('.')
            .is_some_and(|(_, method)| method == "getenv"),
        _ => false,
    };
    if environment {
        return Some("env_var");
    }
    if matches!(language, "kotlin" | "scala") && name == "System.getProperty" {
        return Some("config_key");
    }
    let (receiver, method) = name.rsplit_once('.')?;
    crate::code::feature_flags::extractors::is_config_reader(receiver, method)
        .then_some("config_key")
}

pub(super) fn default_argument<'a, 'b>(language: &str, call: &'b Call<'a>) -> Option<&'b Node<'a>> {
    let supported = match language {
        "python" => matches!(call.name.as_str(), "os.getenv" | "os.environ.get"),
        "ruby" => call.name == "ENV.fetch",
        "starlark" => call.name.ends_with(".getenv"),
        "scala" => matches!(
            call.name.as_str(),
            "sys.env.getOrElse" | "System.getProperty"
        ),
        "kotlin" => call.name == "System.getProperty",
        _ => false,
    } || reader_namespace(language, &call.name) == Some("config_key")
        && !matches!(call.name.as_str(), "System.getenv");
    supported.then(|| call.arguments.get(1)).flatten()
}

pub(super) fn shadowed_reader(
    name: &str,
    node: Node<'_>,
    declarations: &BTreeMap<String, Vec<Node<'_>>>,
    functions: &BTreeMap<String, Vec<Node<'_>>>,
    source: &str,
) -> bool {
    let first = name.split('.').next().unwrap_or(name);
    if scopes::parameter_shadows(node, first, source) {
        return true;
    }
    if declarations
        .get(first)
        .is_some_and(|rows| rows.iter().any(|d| visible(*d, node)))
        || functions.contains_key(first)
    {
        return true;
    }
    let mut ancestor = node.parent();
    for _ in 0..128 {
        let Some(current) = ancestor else {
            break;
        };
        if is_function(current.kind()) {
            if let Some(params) = current.child_by_field_name("parameters") {
                let mut cursor = params.walk();
                if params.named_children(&mut cursor).any(|p| {
                    text(p, source) == first
                        || p.child_by_field_name("name")
                            .is_some_and(|n| text(n, source) == first)
                }) {
                    return true;
                }
            }
        }
        ancestor = current.parent();
    }
    false
}

pub(super) fn module(input: &FeatureFlagFileInput<'_>) -> String {
    let family = match input.language_id {
        "jsx" => "javascript",
        "tsx" => "typescript",
        other => other,
    };
    let mut path = if matches!(
        input.language_id,
        "javascript" | "jsx" | "typescript" | "tsx" | "c" | "cpp" | "starlark" | "php"
    ) {
        input.path.to_owned()
    } else {
        crate::code::languages::strip_supported_extension(input.path).to_owned()
    };
    if input.language_id == "rust" {
        path = path
            .strip_suffix("/lib")
            .or_else(|| path.strip_suffix("/main"))
            .or_else(|| path.strip_suffix("/mod"))
            .unwrap_or(&path)
            .to_owned();
    }
    let mut package = String::new();
    if let Some(root) = input.syntax_root {
        let mut cursor = root.walk();
        for node in root.named_children(&mut cursor).take(1024) {
            if matches!(
                node.kind(),
                "package_clause"
                    | "package_header"
                    | "package_declaration"
                    | "namespace_definition"
                    | "file_scoped_namespace_declaration"
            ) {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .or_else(|| node.named_child(0))
                {
                    package = text(name, input.content).replace('\\', ".");
                }
            }
        }
    }
    if matches!(input.language_id, "go" | "swift") {
        path = format!(
            "{}:{package}",
            input.path.rsplit_once('/').map_or("", |(dir, _)| dir)
        );
    }
    if matches!(input.language_id, "kotlin" | "scala") {
        path = package;
    }
    if input.language_id == "csharp" {
        path = String::new();
    }
    format!("{family}|{path}")
}
