use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn architecture_layer(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let segments = path_segments(&lower);
    if has_segment(&segments, &["test", "tests"]) || lower.contains("_test.") {
        "tests"
    } else if has_segment(&segments, &["docs"]) || lower.ends_with(".md") {
        "docs"
    } else if has_segment(&segments, &["interface", "interfaces", "cli", "web"]) {
        "interfaces"
    } else if has_segment(&segments, &["application", "service", "services"]) {
        "application"
    } else if has_segment(&segments, &["domain", "model", "models"]) {
        "domain"
    } else if has_segment(&segments, &["storage", "repository", "repositories"]) {
        "storage"
    } else if has_segment(&segments, &["net", "network", "http"]) {
        "network"
    } else if lower.contains("config") || lower.ends_with(".toml") || lower.ends_with(".yaml") {
        "configuration"
    } else {
        "source"
    }
}

pub(super) fn layer_confidence(layer: &str) -> f64 {
    match layer {
        "source" => 0.52,
        "docs" | "configuration" => 0.64,
        _ => 0.78,
    }
}

pub(super) fn route_domain(url: &str) -> Option<String> {
    url.trim_matches('/')
        .split('/')
        .find(|segment| {
            let lower = segment.to_ascii_lowercase();
            !segment.is_empty()
                && !segment.starts_with(':')
                && !segment.starts_with('{')
                && !is_route_domain_prefix(&lower)
        })
        .map(normalize_label)
}

pub(super) fn path_domain(path: &str) -> Option<String> {
    let parts = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut start = parts
        .iter()
        .position(|segment| {
            let lower = segment.to_ascii_lowercase();
            !is_source_root_segment(&lower)
        })
        .unwrap_or(0);
    if parts
        .get(start + 1)
        .is_some_and(|segment| is_module_boundary(segment))
    {
        start += 1;
    }
    parts
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, segment)| {
            let lower = segment.to_ascii_lowercase();
            (is_domain_segment(&lower) && !is_path_domain_prefix(&lower))
                .then(|| domain_segment_label(segment, index + 1 == parts.len()))
        })
}

pub(super) fn normalized_view_paths(paths: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for path in paths {
        let path = normalize_view_path(path);
        if path != "." && !path.is_empty() && !normalized.iter().any(|existing| existing == &path) {
            normalized.push(path);
        }
    }
    normalized
}

pub(super) fn domain_token(value: &str) -> Option<String> {
    feature_flag_domain_parts(value)
        .into_iter()
        .find(|part| {
            let lower = part.to_ascii_lowercase();
            lower.len() > 2 && !is_feature_flag_domain_prefix(&lower)
        })
        .map(|part| normalize_label(&part))
}

pub(super) fn domain_confidence(evidence_count: usize) -> f64 {
    (0.45 + (evidence_count.min(5) as f64 * 0.08)).min(0.85)
}

pub(super) fn module_key(path: &str) -> String {
    let parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [] => "root".to_owned(),
        [single] => (*single).to_owned(),
        [workspace, package, ..] if is_workspace_container_segment(workspace) => {
            normalize_label(package)
        }
        [first, _crate_dir, third, ..]
            if matches!(*first, "src" | "app" | "lib") && is_module_boundary(third) =>
        {
            (*third).to_owned()
        }
        ["src", second, ..] | ["app", second, ..] | ["lib", second, ..] => (*second).to_owned(),
        [first, second, ..] if matches!(*first, "src" | "app" | "lib") => (*second).to_owned(),
        [first, ..] => (*first).to_owned(),
    }
}

pub(super) fn affected_candidate_matches_changed_path(
    changed_path: &str,
    candidate_path: &str,
) -> bool {
    module_key(changed_path) == module_key(candidate_path)
        || same_changed_file_parent(changed_path, candidate_path)
}

fn is_module_boundary(segment: &str) -> bool {
    matches!(
        segment,
        "api"
            | "application"
            | "code"
            | "domain"
            | "env"
            | "evaluation"
            | "indexing"
            | "interfaces"
            | "interface"
            | "net"
            | "observability"
            | "paths"
            | "project"
            | "retrieval"
            | "storage"
            | "watcher"
    )
}

fn is_route_domain_prefix(segment: &str) -> bool {
    segment == "api" || is_version_segment(segment)
}

fn is_path_domain_prefix(segment: &str) -> bool {
    matches!(
        segment,
        "api"
            | "controller"
            | "controllers"
            | "endpoint"
            | "endpoints"
            | "graphql"
            | "handler"
            | "handlers"
            | "http"
            | "rest"
            | "route"
            | "routes"
            | "rpc"
            | "web"
    ) || is_version_segment(segment)
}

fn is_feature_flag_domain_prefix(segment: &str) -> bool {
    matches!(
        segment,
        "allow"
            | "allowed"
            | "enable"
            | "enabled"
            | "disable"
            | "disabled"
            | "flag"
            | "has"
            | "is"
            | "rollout"
            | "should"
            | "toggle"
            | "use"
            | "uses"
    )
}

fn is_version_segment(segment: &str) -> bool {
    segment.len() > 1
        && segment.starts_with('v')
        && segment[1..]
            .chars()
            .all(|character| character.is_ascii_digit())
}

fn is_source_root_segment(segment: &str) -> bool {
    matches!(
        segment,
        "src" | "lib" | "app" | "tests" | "test" | "docs" | "bin"
    )
}

fn is_workspace_container_segment(segment: &str) -> bool {
    matches!(
        segment,
        "apps" | "crates" | "modules" | "packages" | "services" | "workspaces"
    )
}

fn is_domain_segment(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    !is_entrypoint_or_metadata_file(&lower) && !is_source_root_segment(&lower) && lower.len() > 2
}

fn is_entrypoint_or_metadata_file(segment: &str) -> bool {
    matches!(
        segment,
        "__init__.py"
            | "cargo.toml"
            | "go.mod"
            | "index.js"
            | "index.jsx"
            | "index.ts"
            | "index.tsx"
            | "lib.rs"
            | "main.rs"
            | "mod.rs"
            | "package.json"
            | "pom.xml"
            | "pyproject.toml"
            | "readme.md"
            | "setup.py"
    )
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn has_segment(segments: &[&str], candidates: &[&str]) -> bool {
    segments
        .iter()
        .any(|segment| candidates.iter().any(|candidate| segment == candidate))
}

fn domain_segment_label(segment: &str, terminal: bool) -> String {
    if terminal
        && let Some((stem, _extension)) = segment.rsplit_once('.')
        && !stem.is_empty()
    {
        return normalize_label(stem);
    }
    normalize_label(segment)
}

fn feature_flag_domain_parts(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for part in value.split(|character: char| !character.is_ascii_alphanumeric()) {
        push_camel_case_parts(part, &mut parts);
    }
    parts
}

fn push_camel_case_parts(value: &str, parts: &mut Vec<String>) {
    let mut current = String::new();
    let mut previous_was_lower_or_digit = false;
    for character in value.chars() {
        if !character.is_ascii_alphanumeric() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            previous_was_lower_or_digit = false;
            continue;
        }
        if character.is_ascii_uppercase() && previous_was_lower_or_digit && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        previous_was_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
        current.push(character);
    }
    if !current.is_empty() {
        parts.push(current);
    }
}

pub(super) fn topological_tour(
    modules: &BTreeSet<String>,
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> (Vec<String>, bool) {
    let mut indegree = modules
        .iter()
        .map(|module| (module.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    for targets in graph.values() {
        for target in targets {
            *indegree.entry(target.clone()).or_default() += 1;
        }
    }
    let mut queue = indegree
        .iter()
        .filter_map(|(module, degree)| (*degree == 0).then_some(module.clone()))
        .collect::<VecDeque<_>>();
    let mut order = Vec::new();
    let mut graph = graph.clone();
    while let Some(module) = queue.pop_front() {
        order.push(module.clone());
        if let Some(targets) = graph.remove(&module) {
            for target in targets {
                if let Some(degree) = indegree.get_mut(&target) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        queue.push_back(target);
                    }
                }
            }
        }
    }
    let cycle = order.len() < modules.len();
    if cycle {
        for module in modules {
            if !order.contains(module) {
                order.push(module.clone());
            }
        }
    }

    (order, cycle)
}

pub(super) fn is_test_config_or_doc(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("test")
        || lower.contains("spec")
        || lower.contains("config")
        || lower.ends_with(".md")
        || lower.ends_with(".yaml")
        || lower.ends_with(".toml")
}

fn normalize_label(value: &str) -> String {
    value
        .trim_matches(|character: char| !character.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

fn same_changed_file_parent(changed_path: &str, candidate_path: &str) -> bool {
    let (changed_parent, changed_name) =
        changed_path.rsplit_once('/').unwrap_or(("", changed_path));
    let (candidate_parent, _) = candidate_path
        .rsplit_once('/')
        .unwrap_or(("", candidate_path));
    let changed_is_file =
        changed_name.contains('.') || changed_name.bytes().any(|byte| byte.is_ascii_uppercase());
    changed_is_file && changed_parent == candidate_parent
}

fn normalize_view_path(path: &str) -> String {
    let mut path = path.replace('\\', "/");
    while path.ends_with('/') {
        path.pop();
    }
    while path.starts_with("./") {
        path.drain(..2);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::{
        affected_candidate_matches_changed_path, architecture_layer, domain_token, module_key,
        path_domain, route_domain, topological_tour,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn architecture_layer_uses_path_boundaries() {
        assert_eq!(
            architecture_layer("src/relay_knowledge/application/service.rs"),
            "application"
        );
        assert_eq!(
            architecture_layer("src/relay_knowledge/storage/sqlite/code.rs"),
            "storage"
        );
        assert_eq!(architecture_layer("tests/relay_knowledge/main.rs"), "tests");
        assert_eq!(
            architecture_layer("src/application/client.rs"),
            "application"
        );
        assert_eq!(architecture_layer("src/webhook/handler.rs"), "source");
    }

    #[test]
    fn domain_rules_skip_generic_roots_and_api_prefixes() {
        assert_eq!(route_domain("/api/v1/users"), Some("users".to_owned()));
        assert_eq!(route_domain("/api/orders"), Some("orders".to_owned()));
        assert_eq!(
            path_domain("src/relay_knowledge/application/service.rs"),
            Some("application".to_owned())
        );
        assert_eq!(path_domain("src/api/users.rs"), Some("users".to_owned()));
        assert_eq!(
            path_domain("app/controllers/orders.py"),
            Some("orders".to_owned())
        );
        assert_eq!(
            path_domain("src/orders/service.rs"),
            Some("orders".to_owned())
        );
        assert_eq!(path_domain("src/users.rs"), Some("users".to_owned()));
        assert_eq!(path_domain("app/orders.py"), Some("orders".to_owned()));
        assert_eq!(path_domain("src/lib.rs"), None);
        assert_eq!(path_domain("src/mod.rs"), None);
        assert_eq!(path_domain("package.json"), None);
    }

    #[test]
    fn domain_tokens_skip_boolean_feature_flag_prefixes() {
        assert_eq!(domain_token("enable_payments"), Some("payments".to_owned()));
        assert_eq!(domain_token("enablePayments"), Some("payments".to_owned()));
        assert_eq!(domain_token("use_checkout"), Some("checkout".to_owned()));
        assert_eq!(domain_token("useCheckout"), Some("checkout".to_owned()));
        assert_eq!(domain_token("is_orders_enabled"), Some("orders".to_owned()));
        assert_eq!(domain_token("isOrdersEnabled"), Some("orders".to_owned()));
        assert_eq!(
            domain_token("rollout.billing.v2"),
            Some("billing".to_owned())
        );
    }

    #[test]
    fn module_keys_skip_common_source_roots() {
        assert_eq!(module_key("src/application/service.rs"), "application");
        assert_eq!(
            module_key("src/relay_knowledge/application/service.rs"),
            "application"
        );
        assert_eq!(module_key("crates/auth/src/lib.rs"), "auth");
        assert_eq!(module_key("packages/api/src/routes.ts"), "api");
        assert_eq!(module_key("docs/en/index.md"), "docs");
    }

    #[test]
    fn affected_candidate_matches_root_changed_file_siblings() {
        assert!(affected_candidate_matches_changed_path(
            "Cargo.toml",
            "Cargo.lock"
        ));
        assert!(affected_candidate_matches_changed_path(
            "package.json",
            "README.md"
        ));
        assert!(!affected_candidate_matches_changed_path(
            "Cargo.toml",
            "src/lib.rs"
        ));
    }

    #[test]
    fn topological_tour_reports_cycles() {
        let modules = ["a".to_owned(), "b".to_owned()]
            .into_iter()
            .collect::<BTreeSet<_>>();
        let graph = BTreeMap::from([
            ("a".to_owned(), BTreeSet::from(["b".to_owned()])),
            ("b".to_owned(), BTreeSet::from(["a".to_owned()])),
        ]);

        let (tour, cycle) = topological_tour(&modules, &graph);

        assert!(cycle);
        assert_eq!(tour.len(), 2);
    }
}
