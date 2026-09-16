//! Explicit import bindings; unknown package layouts retain unresolved identities.
use super::*;

pub(super) fn rust_bindings(
    node: Node<'_>,
    source: &str,
) -> Result<Vec<(String, String)>, DomainError> {
    let mut pending = vec![(node, String::new())];
    let mut bindings = Vec::new();
    let mut visited = 0;
    while let Some((node, prefix)) = pending.pop() {
        visited += 1;
        if visited > 1024 {
            return Err(DomainError::invalid(
                "configuration",
                "Rust import binding budget exceeded",
            ));
        }
        match node.kind() {
            "use_declaration" => {
                if let Some(argument) = node.child_by_field_name("argument") {
                    pending.push((argument, prefix));
                }
            }
            "use_list" => {
                let mut cursor = node.walk();
                for child in node
                    .named_children(&mut cursor)
                    .filter(|child| !child.is_extra())
                {
                    if pending.len() >= 1024 {
                        return Err(DomainError::invalid(
                            "configuration",
                            "Rust import binding budget exceeded",
                        ));
                    }
                    pending.push((child, prefix.clone()));
                }
            }
            "scoped_use_list" => {
                if let (Some(path), Some(list)) = (
                    node.child_by_field_name("path")
                        .and_then(|path| rust_use_path(path, source, 0)),
                    node.child_by_field_name("list"),
                ) {
                    pending.push((list, format!("{prefix}{path}::")));
                } else {
                    bindings.push(("*".into(), "unresolved".into()));
                }
            }
            "use_wildcard" => {
                let path = node
                    .named_child(0)
                    .and_then(|path| rust_use_path(path, source, 0))
                    .unwrap_or_default();
                let path = format!("{prefix}{path}");
                bindings.push(("*".into(), format!("{}::*", path.trim_end_matches("::"))));
            }
            _ => {
                let (path, alias) = if node.kind() == "use_as_clause" {
                    (
                        node.child_by_field_name("path"),
                        node.child_by_field_name("alias")
                            .map(|n| syntax::text(n, source)),
                    )
                } else {
                    (Some(node), None)
                };
                let Some(path) = path.and_then(|path| rust_use_path(path, source, 0)) else {
                    bindings.push(("*".into(), "unresolved".into()));
                    continue;
                };
                let target = if path == "self" {
                    prefix.trim_end_matches("::").to_owned()
                } else {
                    format!("{prefix}{path}")
                };
                let alias = alias.unwrap_or_else(|| target.rsplit("::").next().unwrap_or_default());
                bindings.push((alias.to_owned(), target));
            }
        }
    }
    Ok(bindings)
}

fn rust_use_path(node: Node<'_>, source: &str, depth: usize) -> Option<String> {
    if depth >= MAX_DEPTH {
        return None;
    }
    match node.kind() {
        "identifier" | "self" | "super" | "crate" => Some(syntax::text(node, source).to_owned()),
        "scoped_identifier" => {
            let name = rust_use_path(node.child_by_field_name("name")?, source, depth + 1)?;
            if let Some(path) = node.child_by_field_name("path") {
                Some(format!(
                    "{}::{name}",
                    rust_use_path(path, source, depth + 1)?
                ))
            } else {
                Some(format!("::{name}"))
            }
        }
        _ => None,
    }
}

impl Analysis<'_> {
    /// A familiar spelling is insufficient when an explicit import replaced
    /// the standard owner. Imports outside this lexical scope do not bind it.
    pub(super) fn reader_import_shadowed(&self, name: &str, use_site: Node<'_>) -> bool {
        // `import.meta` is grammar syntax, not a lexical binding named import.
        if name.starts_with("import.meta.") {
            return false;
        }
        let root = name.split('.').next().unwrap_or(name);
        if self.input.language_id == "rust" {
            return self.rust_reader_shadowed(root, use_site);
        }
        self.imports.iter().any(|node| {
            if !syntax::visible_function(*node, use_site, "", self.input.content) {
                return false;
            }
            let raw = syntax::text(*node, self.input.content);
            match self.input.language_id {
                "python" => {
                    if let Some(imports) = raw.strip_prefix("import ") {
                        imports.split(',').any(|part| {
                            let (target, alias) = part
                                .trim()
                                .split_once(" as ")
                                .unwrap_or((part.trim(), part.trim()));
                            alias == root && target != root
                        })
                    } else if let Some((module, imports)) = raw
                        .strip_prefix("from ")
                        .and_then(|s| s.split_once(" import "))
                    {
                        imports.split(',').any(|part| {
                            let (target, alias) = part
                                .trim()
                                .split_once(" as ")
                                .unwrap_or((part.trim(), part.trim()));
                            alias == root && !(module == "builtins" && target == root)
                        })
                    } else {
                        false
                    }
                }
                "javascript" | "jsx" | "typescript" | "tsx" => {
                    raw.split(" from ").next().is_some_and(|head| {
                        head.split(|c: char| !c.is_alphanumeric() && c != '_')
                            .any(|word| word == root)
                    })
                }
                "kotlin" | "scala" | "csharp" => {
                    if let Some((alias, target)) =
                        native_alias(*node, self.input.language_id, self.input.content)
                    {
                        return alias == root && target != root;
                    }
                    let tokens: Vec<_> = raw
                        .trim_end_matches(';')
                        .split(['.', ' ', '='])
                        .filter(|s| !s.is_empty())
                        .collect();
                    tokens.last() == Some(&root)
                        && !raw.contains("java.lang.System")
                        && !raw.contains("System.Environment")
                }
                _ => false,
            }
        })
    }

    fn rust_reader_shadowed(&self, root: &str, mut site: Node<'_>) -> bool {
        for _ in 0..128 {
            let declarations = self.declarations.get(root).into_iter().flatten();
            if declarations
                .filter(|n| n.kind() == "mod_item")
                .any(|n| syntax::scope(*n) == site)
            {
                return true;
            }
            let named = self
                .rust_imports
                .get(root)
                .into_iter()
                .flatten()
                .filter(|(n, _)| syntax::scope(*n) == site)
                .collect::<Vec<_>>();
            let bindings = if named.is_empty() {
                self.rust_imports
                    .get("*")
                    .into_iter()
                    .flatten()
                    .filter(|(n, _)| syntax::scope(*n) == site)
                    .collect::<Vec<_>>()
            } else {
                named
            };
            if !bindings.is_empty() {
                return bindings.iter().any(|(node, target)| {
                    let standard = target.trim_start_matches("::");
                    let expected = if root == "std" {
                        "std".into()
                    } else {
                        format!("std::{root}")
                    };
                    if standard != expected
                        && !(standard == "std::*" && matches!(root, "env" | "std"))
                    {
                        return true;
                    }
                    // An absolute import names the external crate; a relative
                    // std path can instead name a repository-local module.
                    !target.starts_with("::")
                        && self
                            .declarations
                            .get("std")
                            .into_iter()
                            .flatten()
                            .any(|declaration| {
                                declaration.kind() == "mod_item"
                                    && syntax::visible_function(
                                        *declaration,
                                        *node,
                                        "",
                                        self.input.content,
                                    )
                            })
                });
            }
            let Some(parent) = site.parent() else {
                return false;
            };
            site = parent;
        }
        true
    }
}

pub(super) fn imported_binding(
    analysis: &Analysis<'_>,
    spelling: &str,
    use_site: Node<'_>,
) -> Option<String> {
    let mut candidates = BTreeSet::new();
    let name = spelling.trim().trim_start_matches('$').replace("::", ".");
    if analysis.binding_shadowed(use_site, &name) {
        return None;
    }
    for node in &analysis.imports {
        if !syntax::visible(*node, use_site) {
            continue;
        }
        let raw = syntax::text(*node, analysis.input.content);
        match (analysis.input.language_id, node.kind()) {
            ("python", "import_statement") => {
                let Some(imports) = raw.strip_prefix("import ") else {
                    continue;
                };
                for item in imports.split(',') {
                    let mut parts = item.split_whitespace();
                    let Some(module) = parts.next() else {
                        continue;
                    };
                    let alias = if parts.next() == Some("as") {
                        parts.next().unwrap_or(module)
                    } else {
                        module
                    };
                    if let Some(member) = name.strip_prefix(&format!("{alias}.")) {
                        candidates.insert(format!("python|{}||{member}", module.replace('.', "/")));
                    }
                }
            }
            ("python" | "starlark", "import_from_statement") => {
                let (module, imports) = raw.strip_prefix("from ")?.split_once(" import ")?;
                for item in imports.trim_matches(['(', ')']).split(',') {
                    let mut parts = item.split_whitespace();
                    let Some(original) = parts.next() else {
                        continue;
                    };
                    let alias = if parts.next() == Some("as") {
                        parts.next().unwrap_or(original)
                    } else {
                        original
                    };
                    if name == alias {
                        if let Some(path) = python_module(analysis.input.path, module) {
                            candidates.insert(format!("python|{path}||{original}"));
                        }
                    }
                }
            }
            ("javascript" | "jsx" | "typescript" | "tsx", "import_statement") => {
                let Some(source_node) = node.child_by_field_name("source") else {
                    continue;
                };
                let specifier =
                    syntax::text(source_node, analysis.input.content).trim_matches(['\'', '"']);
                let Some(path) = relative_module(analysis.input.path, specifier) else {
                    continue;
                };
                let head = raw.strip_prefix("import ")?.split(" from ").next()?;
                let family = if matches!(analysis.input.language_id, "typescript" | "tsx") {
                    "typescript"
                } else {
                    "javascript"
                };
                let head = head.trim();
                if !head.starts_with(['{', '*']) {
                    let default_alias = head.split(',').next().unwrap_or(head).trim();
                    if name == default_alias {
                        candidates.insert(format!("{family}|{path}||default"));
                    }
                }
                let named = if let Some((_, named)) = head.split_once('{') {
                    named.trim_end_matches('}').trim()
                } else if head.starts_with('*') {
                    head
                } else {
                    ""
                };
                for item in named.split(',') {
                    let mut parts = item.split_whitespace();
                    let Some(original) = parts.next() else {
                        continue;
                    };
                    let alias = if parts.next() == Some("as") {
                        parts.next().unwrap_or(original)
                    } else {
                        original
                    };
                    if name == alias {
                        candidates.insert(format!("{family}|{path}||{original}"));
                    }
                    if original == "*"
                        && let Some(member) = name.strip_prefix(&format!("{alias}."))
                    {
                        candidates.insert(format!("{family}|{path}||{member}"));
                    }
                }
            }
            ("c" | "cpp", "preproc_include") => {
                let Some(path_node) = node.child_by_field_name("path") else {
                    continue;
                };
                let path = syntax::text(path_node, analysis.input.content);
                if let Some(path) = path
                    .strip_prefix('"')
                    .and_then(|p| p.strip_suffix('"'))
                    .and_then(|p| relative_module(analysis.input.path, &format!("./{p}")))
                {
                    candidates.insert(format!("{}|{path}||{name}", analysis.input.language_id));
                }
            }
            (
                "php",
                "require_expression"
                | "require_once_expression"
                | "include_expression"
                | "include_once_expression",
            ) => {
                let Some(value) = node.named_child(0) else {
                    continue;
                };
                if value.kind() != "binary_expression" {
                    continue;
                }
                let (Some(left), Some(right)) = (
                    value.child_by_field_name("left"),
                    value.child_by_field_name("right"),
                ) else {
                    continue;
                };
                if syntax::text(left, analysis.input.content) != "__DIR__"
                    || analysis.input.content[left.end_byte()..right.start_byte()].trim() != "."
                {
                    continue;
                }
                let Some(Value::Literal(suffix)) =
                    analysis.evaluate(right, 0, &mut BTreeSet::new())
                else {
                    continue;
                };
                if suffix.kind != "string" || !suffix.text.starts_with('/') {
                    continue;
                }
                if let Some(path) =
                    relative_module(analysis.input.path, &format!(".{}", suffix.text))
                {
                    let (owner, member) = name.rsplit_once('.').unwrap_or(("", &name));
                    candidates.insert(format!("php|{path}|{owner}|{member}"));
                }
            }
            ("rust", "use_declaration") => {
                let Some(import) = raw.strip_prefix("use ").and_then(|s| s.strip_suffix(';'))
                else {
                    continue;
                };
                if import.contains(['{', '*']) {
                    continue;
                }
                let (target, alias) = import
                    .split_once(" as ")
                    .unwrap_or((import, import.rsplit("::").next()?));
                let (target, member) = if let Some(member) = name.strip_prefix(&format!("{alias}."))
                {
                    (target, Some(member))
                } else if name == alias {
                    (target, None)
                } else {
                    continue;
                };
                let target = member
                    .map_or_else(|| target.to_owned(), |member| format!("{target}::{member}"));
                if target.starts_with("crate::") || target.starts_with("self::") {
                    candidates.insert(format!(
                        "rust-import|{}|{}",
                        analysis.input.path,
                        target.replace("::", ".")
                    ));
                }
            }
            ("ruby", "call") => {
                let Some(call) = syntax::call(*node, analysis.input.content)
                    .filter(|c| c.name == "require_relative")
                else {
                    continue;
                };
                let Some(module) = call.arguments.first() else {
                    continue;
                };
                let module =
                    syntax::text(*module, analysis.input.content).trim_matches(['\'', '"']);
                if module.contains(['#', '\\']) {
                    continue;
                }
                if let Some(path) = relative_module(analysis.input.path, &format!("./{module}")) {
                    let (owner, member) = name.rsplit_once('.').unwrap_or(("", &name));
                    candidates.insert(format!("ruby|{path}|{owner}|{member}"));
                }
            }
            ("starlark", "call") => {
                let Some(call) =
                    syntax::call(*node, analysis.input.content).filter(|c| c.name == "load")
                else {
                    continue;
                };
                let Some(module) = call.arguments.first() else {
                    continue;
                };
                let raw_module =
                    syntax::text(*module, analysis.input.content).trim_matches(['\'', '"']);
                let path = if let Some(path) = raw_module.strip_prefix("//") {
                    Some(path.replace(':', "/"))
                } else {
                    relative_module(
                        analysis.input.path,
                        &format!("./{}", raw_module.trim_start_matches(':')),
                    )
                };
                let Some(path) = path else {
                    continue;
                };
                for symbol in call.arguments.iter().skip(1) {
                    let raw_symbol = syntax::text(*symbol, analysis.input.content);
                    let (alias, original) = raw_symbol
                        .split_once('=')
                        .map_or((raw_symbol, raw_symbol), |(a, b)| (a.trim(), b.trim()));
                    if name == alias.trim_matches(['\'', '"']) {
                        candidates.insert(format!(
                            "starlark|{path}||{}",
                            original.trim_matches(['\'', '"'])
                        ));
                    }
                }
            }
            ("csharp", "using_directive") | ("kotlin", "import_header" | "import") => {
                if let Some((alias, target)) =
                    native_alias(*node, analysis.input.language_id, analysis.input.content)
                    && (name == alias || name.starts_with(&format!("{alias}.")))
                {
                    let reference = if analysis.input.language_id == "csharp" {
                        name.strip_prefix(&format!("{alias}.")).and_then(|suffix| {
                            let target = if let Some(global) = target.strip_prefix("global::") {
                                global.to_owned()
                            } else {
                                let namespace =
                                    scopes::csharp_namespace(*node, analysis.input.content);
                                if namespace.is_empty() {
                                    target.clone()
                                } else {
                                    format!("{namespace}.{target}")
                                }
                            };
                            let expanded = format!("{target}.{suffix}");
                            if !expanded.split('.').all(|part| {
                                !part.is_empty()
                                    && part.chars().all(|c| c.is_alphanumeric() || c == '_')
                            }) {
                                return None;
                            }
                            let (owner, member) = expanded.rsplit_once('.')?;
                            Some(format!("csharp||{owner}|{member}"))
                        })
                    } else {
                        None
                    };
                    candidates.insert(reference.unwrap_or_else(|| {
                        format!(
                            "unresolved-import|{}|{}|{name}",
                            analysis.input.language_id, analysis.input.path
                        )
                    }));
                }
            }
            ("scala", "import_declaration") => {
                // Selector, wildcard and renamed imports can replace native
                // package lookup. Preserve a blocker until the selector's
                // module ownership can be proved from indexed evidence.
                candidates.insert(format!(
                    "unresolved-import|scala|{}|{name}",
                    analysis.input.path
                ));
            }
            _ => {}
        }
    }
    if candidates.is_empty()
        && matches!(
            analysis.input.language_id,
            "go" | "kotlin" | "scala" | "csharp" | "swift"
        )
        && name
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_'))
    {
        let (owner, member) = name.rsplit_once('.').unwrap_or(("", &name));
        // The module key includes native package/namespace evidence. Storage
        // accepts this reference only when that exact provider was indexed.
        let namespace = if analysis.input.language_id == "csharp" {
            scopes::csharp_namespace(use_site, analysis.input.content)
        } else {
            String::new()
        };
        let owner = if namespace.is_empty() {
            owner.to_owned()
        } else if owner.is_empty() {
            namespace
        } else {
            format!("{namespace}.{owner}")
        };
        candidates.insert(format!("{}|{owner}|{member}", analysis.module));
    }
    (candidates.len() == 1)
        .then(|| candidates.into_iter().next())
        .flatten()
}

fn native_alias(node: Node<'_>, language: &str, source: &str) -> Option<(String, String)> {
    if !matches!(
        (language, node.kind()),
        ("csharp", "using_directive") | ("kotlin", "import" | "import_header")
    ) {
        return None;
    }
    let mut cursor = node.walk();
    let children = node
        .named_children(&mut cursor)
        .filter(|n| !n.is_extra())
        .take(3)
        .collect::<Vec<_>>();
    if language == "csharp" {
        let [alias, target] = children.as_slice() else {
            return None;
        };
        let mut cursor = node.walk();
        if alias.kind() != "identifier" || !node.children(&mut cursor).any(|n| n.kind() == "=") {
            return None;
        }
        return Some((
            syntax::text(*alias, source).to_owned(),
            syntax::text(*target, source).to_owned(),
        ));
    }
    match children.as_slice() {
        [target, alias] => Some((
            syntax::text(*alias, source).to_owned(),
            syntax::text(*target, source).to_owned(),
        )),
        [target] => {
            let target = syntax::text(*target, source);
            Some((target.rsplit('.').next()?.to_owned(), target.to_owned()))
        }
        _ => None,
    }
}

fn python_module(path: &str, module: &str) -> Option<String> {
    if !module.starts_with('.') {
        return Some(module.replace('.', "/"));
    }
    let levels = module.chars().take_while(|c| *c == '.').count();
    let mut base = path
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    for _ in 1..levels {
        base.pop()?;
    }
    let suffix = module[levels..].replace('.', "/");
    Some(if base.is_empty() {
        suffix
    } else {
        format!("{}/{suffix}", base.join("/"))
    })
}

fn relative_module(path: &str, module: &str) -> Option<String> {
    if !module.starts_with('.') {
        return None;
    }
    let mut parts = path
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .split('/')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for part in module.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other.to_owned()),
        }
    }
    let joined = parts.join("/");
    Some(joined)
}

#[cfg(test)]
mod mod_tests {
    use super::*;
    #[test]
    fn resolves_only_repository_relative_module_paths() {
        assert_eq!(
            relative_module("src/demo.ts", "./settings.ts").as_deref(),
            Some("src/settings.ts")
        );
        assert_eq!(relative_module("src/demo.ts", "../../outside"), None);
        assert_eq!(relative_module("src/demo.ts", "package"), None);
        assert_eq!(
            python_module("pkg/demo.py", ".settings").as_deref(),
            Some("pkg/settings")
        );
    }
}
