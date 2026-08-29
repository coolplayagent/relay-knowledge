use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use proc_macro2::Span;
use serde_json::json;
use syn::{
    Attribute, File, Item, ItemUse, Path as SynPath, UseTree,
    visit::{self, Visit},
};

use super::{production_rust_files, relative_source_path, source_root};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DependencyReference {
    source_module: String,
    target_module: String,
    path: String,
    line: usize,
}

#[derive(Debug, Clone, Copy)]
struct EdgeAllowance {
    path: &'static str,
    target_module: &'static str,
    max_references: usize,
}

const APPLICATION_INFRASTRUCTURE_MODULES: &[&str] = &["env", "net", "paths"];

// Existing process-configuration coupling is an explicit migration budget. The
// syntax graph counts grouped imports and qualified paths, so any new reference
// fails even when it is hidden inside `use crate::{ ... }`.
const APPLICATION_INFRASTRUCTURE_BASELINE: &[EdgeAllowance] = &[
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/agent.rs",
        target_module: "env",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/file_index.rs",
        target_module: "env",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/file_index.rs",
        target_module: "paths",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/mod.rs",
        target_module: "env",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/mod.rs",
        target_module: "net",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/mod.rs",
        target_module: "paths",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/retrieval.rs",
        target_module: "env",
        max_references: 3,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/storage.rs",
        target_module: "env",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/runtime/worker.rs",
        target_module: "env",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/service/lifecycle_plan/mod.rs",
        target_module: "paths",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/service/lifecycle_plan/platform_service.rs",
        target_module: "env",
        max_references: 2,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/service/mod.rs",
        target_module: "env",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/update/config/mod.rs",
        target_module: "env",
        max_references: 1,
    },
    EdgeAllowance {
        path: "src/relay_knowledge/application/update/workflow/mod.rs",
        target_module: "paths",
        max_references: 1,
    },
];

#[test]
fn module_dependency_graph_is_acyclic_and_respects_layers() {
    let root = source_root();
    let graph = ModuleGraph::load(&root);
    graph.write_reports(&architecture_report_directory());

    let mut violations = graph.architecture_violations();
    violations.extend(
        graph
            .strongly_connected_components()
            .into_iter()
            .filter(|component| component.len() > 1)
            .map(|component| {
                let evidence = graph.cycle_evidence(&component).join("; ");
                format!(
                    "module dependency cycle [{}]: {evidence}",
                    component.join(" -> ")
                )
            }),
    );

    assert!(
        violations.is_empty(),
        "architecture dependency violations:\n{}\nReports: {}",
        violations.join("\n"),
        architecture_report_directory().display()
    );
}

#[test]
fn grouped_imports_are_expanded_by_rust_syntax() {
    let references = references_from_source(
        "application",
        "src/relay_knowledge/application/example.rs",
        "use crate::{domain::GraphVersion, env::{EnvironmentConfig, PlatformKind}, paths::RuntimePaths};",
    );
    let targets = references
        .iter()
        .map(|reference| reference.target_module.as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(targets, BTreeSet::from(["domain", "env", "paths"]));
}

#[test]
fn grouped_application_infrastructure_imports_exceed_the_migration_baseline() {
    let graph = ModuleGraph::from_references(references_from_source(
        "application",
        "src/relay_knowledge/application/example.rs",
        "use crate::{env::EnvironmentConfig, paths::RuntimePaths};",
    ));

    let violations = graph.layer_violations();
    assert!(
        violations.iter().any(|violation| {
            violation.contains("application -> env")
                && violation.contains("migration baseline allows 0")
        }),
        "grouped env import should be rejected: {violations:?}"
    );
    assert!(
        violations.iter().any(|violation| {
            violation.contains("application -> paths")
                && violation.contains("migration baseline allows 0")
        }),
        "grouped paths import should be rejected: {violations:?}"
    );
}

#[test]
fn strongly_connected_components_detect_bootstrap_interface_cycle() {
    let graph = ModuleGraph::from_references(vec![
        reference("bootstrap", "interfaces"),
        reference("interfaces", "bootstrap"),
        reference("interfaces", "application"),
    ]);

    assert!(
        graph
            .strongly_connected_components()
            .iter()
            .any(|component| component == &["bootstrap", "interfaces"])
    );
}

#[test]
fn layer_policy_rejects_api_storage_dependency() {
    let graph = ModuleGraph::from_references(vec![reference("api", "storage")]);

    assert!(
        graph
            .layer_violations()
            .iter()
            .any(|violation| violation.contains("api -> storage"))
    );
}

#[test]
fn layer_policy_treats_identity_as_an_inner_primitive() {
    let graph = ModuleGraph::from_references(vec![reference("domain", "identity")]);

    assert!(graph.layer_violations().is_empty());
}

struct ModuleGraph {
    references: Vec<DependencyReference>,
}

impl ModuleGraph {
    fn load(source_root: &Path) -> Self {
        let mut references = Vec::new();
        for path in production_rust_files(source_root) {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let relative_path = relative_source_path(&path, source_root);
            let source_module = top_level_module(&path, source_root);
            references.extend(references_from_source(
                &source_module,
                &relative_path,
                &source,
            ));
        }
        Self::from_references(references)
    }

    fn from_references(mut references: Vec<DependencyReference>) -> Self {
        references.retain(|reference| reference.source_module != reference.target_module);
        references.sort();
        references.dedup();
        Self { references }
    }

    fn edges(&self) -> BTreeSet<(String, String)> {
        self.references
            .iter()
            .map(|reference| {
                (
                    reference.source_module.clone(),
                    reference.target_module.clone(),
                )
            })
            .collect()
    }

    fn nodes(&self) -> BTreeSet<String> {
        self.edges()
            .into_iter()
            .flat_map(|(source, target)| [source, target])
            .collect()
    }

    fn layer_violations(&self) -> Vec<String> {
        let mut violations = Vec::new();
        for reference in &self.references {
            let reason = forbidden_edge_reason(
                reference.source_module.as_str(),
                reference.target_module.as_str(),
            );
            if let Some(reason) = reason {
                violations.push(format!(
                    "{} -> {} at {}:{}: {reason}",
                    reference.source_module,
                    reference.target_module,
                    reference.path,
                    reference.line
                ));
            }
        }

        for target in APPLICATION_INFRASTRUCTURE_MODULES {
            let grouped = self
                .references
                .iter()
                .filter(|reference| {
                    reference.source_module == "application" && reference.target_module == *target
                })
                .fold(BTreeMap::<&str, usize>::new(), |mut counts, reference| {
                    *counts.entry(reference.path.as_str()).or_default() += 1;
                    counts
                });
            for (path, count) in grouped {
                let allowed = APPLICATION_INFRASTRUCTURE_BASELINE
                    .iter()
                    .find(|allowance| allowance.path == path && allowance.target_module == *target)
                    .map_or(0, |allowance| allowance.max_references);
                if count > allowed {
                    violations.push(format!(
                        "application -> {target} has {count} syntax reference(s) at {path}; migration baseline allows {allowed}"
                    ));
                }
            }
        }

        violations.sort();
        violations.dedup();
        violations
    }

    fn architecture_violations(&self) -> Vec<String> {
        let mut violations = self.layer_violations();
        for allowance in APPLICATION_INFRASTRUCTURE_BASELINE {
            let actual = self
                .references
                .iter()
                .filter(|reference| {
                    reference.source_module == "application"
                        && reference.path == allowance.path
                        && reference.target_module == allowance.target_module
                })
                .count();
            if actual < allowance.max_references {
                violations.push(format!(
                    "stale application -> {} migration baseline at {}: actual {actual}, budget {}; reduce or remove the allowance",
                    allowance.target_module, allowance.path, allowance.max_references
                ));
            }
        }
        violations.sort();
        violations.dedup();
        violations
    }

    fn strongly_connected_components(&self) -> Vec<Vec<String>> {
        let nodes = self.nodes().into_iter().collect::<Vec<_>>();
        let node_indexes = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.clone(), index))
            .collect::<HashMap<_, _>>();
        let mut adjacency = vec![Vec::new(); nodes.len()];
        for (source, target) in self.edges() {
            adjacency[node_indexes[&source]].push(node_indexes[&target]);
        }

        Tarjan::new(&adjacency)
            .components()
            .into_iter()
            .map(|component| {
                let mut names = component
                    .into_iter()
                    .map(|index| nodes[index].clone())
                    .collect::<Vec<_>>();
                names.sort();
                names
            })
            .collect()
    }

    fn cycle_evidence(&self, component: &[String]) -> Vec<String> {
        let members = component
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        self.references
            .iter()
            .filter(|reference| {
                members.contains(reference.source_module.as_str())
                    && members.contains(reference.target_module.as_str())
            })
            .map(|reference| {
                format!(
                    "{} -> {} at {}:{}",
                    reference.source_module,
                    reference.target_module,
                    reference.path,
                    reference.line
                )
            })
            .collect()
    }

    fn write_reports(&self, directory: &Path) {
        fs::create_dir_all(directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        let edges = self.edges();
        let cycles = self
            .strongly_connected_components()
            .into_iter()
            .filter(|component| component.len() > 1)
            .collect::<Vec<_>>();
        let json_report = json!({
            "schema_version": 1,
            "nodes": self.nodes(),
            "edges": edges.iter().map(|(source, target)| json!({
                "source": source,
                "target": target,
                "references": self.references.iter().filter(|reference| {
                    &reference.source_module == source && &reference.target_module == target
                }).map(|reference| json!({
                    "path": reference.path,
                    "line": reference.line,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "cycles": cycles,
            "migration_baselines": {
                "application_infrastructure": APPLICATION_INFRASTRUCTURE_BASELINE.iter().map(|allowance| {
                    let actual = self.references.iter().filter(|reference| {
                        reference.source_module == "application"
                            && reference.path == allowance.path
                            && reference.target_module == allowance.target_module
                    }).count();
                    json!({
                        "path": allowance.path,
                        "target": allowance.target_module,
                        "reference_budget": allowance.max_references,
                        "actual_references": actual,
                    })
                }).collect::<Vec<_>>(),
            },
            "violations": self.architecture_violations(),
        });
        fs::write(
            directory.join("module-graph.json"),
            serde_json::to_vec_pretty(&json_report).expect("architecture JSON should serialize"),
        )
        .unwrap_or_else(|error| panic!("write architecture JSON report: {error}"));

        let mut dot = String::from("digraph relay_knowledge_modules {\n  rankdir=LR;\n");
        for (source, target) in edges {
            dot.push_str(&format!("  \"{source}\" -> \"{target}\";\n"));
        }
        dot.push_str("}\n");
        fs::write(directory.join("module-graph.dot"), dot)
            .unwrap_or_else(|error| panic!("write architecture DOT report: {error}"));
    }
}

fn forbidden_edge_reason(source: &str, target: &str) -> Option<&'static str> {
    match (source, target) {
        ("domain", "identity") => None,
        ("domain", _) => Some("domain must not depend on an outer crate module"),
        (
            "ports",
            "adapters" | "api" | "application" | "bootstrap" | "env" | "interfaces" | "net"
            | "paths" | "storage",
        ) => Some("ports must remain technology-neutral inner contracts"),
        (
            "api",
            "adapters" | "application" | "bootstrap" | "env" | "interfaces" | "net" | "paths"
            | "storage",
        ) => Some("API contracts must use domain/contract types rather than infrastructure"),
        ("application", "adapters" | "bootstrap" | "interfaces") => {
            Some("application must not depend on outer assembly or interface modules")
        }
        ("interfaces", "bootstrap") => {
            Some("interfaces must not call back into the outer bootstrap layer")
        }
        ("storage", "api" | "application" | "bootstrap" | "interfaces") => {
            Some("storage must not depend on use cases or delivery layers")
        }
        _ => None,
    }
}

fn top_level_module(path: &Path, source_root: &Path) -> String {
    let relative = path
        .strip_prefix(source_root)
        .unwrap_or_else(|_| panic!("{} must be below {}", path.display(), source_root.display()));
    let first = relative
        .components()
        .next()
        .expect("Rust source has a relative component")
        .as_os_str()
        .to_string_lossy();
    first.strip_suffix(".rs").unwrap_or(&first).to_owned()
}

fn references_from_source(
    source_module: &str,
    relative_path: &str,
    source: &str,
) -> Vec<DependencyReference> {
    let syntax: File = syn::parse_file(source)
        .unwrap_or_else(|error| panic!("parse Rust syntax from {relative_path}: {error}"));
    let mut collector = DependencyCollector {
        source_module,
        relative_path,
        references: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.references
}

struct DependencyCollector<'a> {
    source_module: &'a str,
    relative_path: &'a str,
    references: Vec<DependencyReference>,
}

impl DependencyCollector<'_> {
    fn record(&mut self, segments: &[String], span: Span) {
        let Some((first, remaining)) = segments.split_first() else {
            return;
        };
        if first != "crate" {
            return;
        }
        let Some(target) = remaining.first() else {
            return;
        };
        self.references.push(DependencyReference {
            source_module: self.source_module.to_owned(),
            target_module: target.clone(),
            path: self.relative_path.to_owned(),
            line: span.start().line.max(1),
        });
    }
}

impl<'ast> Visit<'ast> for DependencyCollector<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attributes(item).is_some_and(is_test_configuration) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        collect_use_tree(self, Vec::new(), &item.tree, item.use_token.span);
    }

    fn visit_path(&mut self, path: &'ast SynPath) {
        let segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        self.record(
            &segments,
            path.segments
                .first()
                .map_or_else(Span::call_site, |segment| segment.ident.span()),
        );
        visit::visit_path(self, path);
    }
}

fn collect_use_tree(
    collector: &mut DependencyCollector<'_>,
    mut prefix: Vec<String>,
    tree: &UseTree,
    fallback_span: Span,
) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_use_tree(collector, prefix, &path.tree, path.ident.span());
        }
        UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            collector.record(&prefix, name.ident.span());
        }
        UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            collector.record(&prefix, rename.ident.span());
        }
        UseTree::Glob(_) => collector.record(&prefix, fallback_span),
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree(
                    collector,
                    prefix.clone(),
                    item,
                    group.brace_token.span.open(),
                );
            }
        }
    }
}

fn item_attributes(item: &Item) -> Option<&[Attribute]> {
    match item {
        Item::Const(item) => Some(&item.attrs),
        Item::Enum(item) => Some(&item.attrs),
        Item::ExternCrate(item) => Some(&item.attrs),
        Item::Fn(item) => Some(&item.attrs),
        Item::ForeignMod(item) => Some(&item.attrs),
        Item::Impl(item) => Some(&item.attrs),
        Item::Macro(item) => Some(&item.attrs),
        Item::Mod(item) => Some(&item.attrs),
        Item::Static(item) => Some(&item.attrs),
        Item::Struct(item) => Some(&item.attrs),
        Item::Trait(item) => Some(&item.attrs),
        Item::TraitAlias(item) => Some(&item.attrs),
        Item::Type(item) => Some(&item.attrs),
        Item::Union(item) => Some(&item.attrs),
        Item::Use(item) => Some(&item.attrs),
        _ => None,
    }
}

fn is_test_configuration(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && match &attribute.meta {
                syn::Meta::List(list) => list.tokens.to_string().split_whitespace().any(|token| {
                    token.trim_matches(|character: char| {
                        !character.is_alphanumeric() && character != '_'
                    }) == "test"
                }),
                _ => false,
            }
    })
}

fn reference(source: &str, target: &str) -> DependencyReference {
    DependencyReference {
        source_module: source.to_owned(),
        target_module: target.to_owned(),
        path: "fixture.rs".to_owned(),
        line: 1,
    }
}

fn architecture_report_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/architecture")
}

struct Tarjan<'a> {
    adjacency: &'a [Vec<usize>],
    next_index: usize,
    indexes: Vec<Option<usize>>,
    low_links: Vec<usize>,
    stack: Vec<usize>,
    on_stack: Vec<bool>,
    components: Vec<Vec<usize>>,
}

impl<'a> Tarjan<'a> {
    fn new(adjacency: &'a [Vec<usize>]) -> Self {
        Self {
            adjacency,
            next_index: 0,
            indexes: vec![None; adjacency.len()],
            low_links: vec![0; adjacency.len()],
            stack: Vec::new(),
            on_stack: vec![false; adjacency.len()],
            components: Vec::new(),
        }
    }

    fn components(mut self) -> Vec<Vec<usize>> {
        for node in 0..self.adjacency.len() {
            if self.indexes[node].is_none() {
                self.connect(node);
            }
        }
        self.components
    }

    fn connect(&mut self, node: usize) {
        let node_index = self.next_index;
        self.next_index += 1;
        self.indexes[node] = Some(node_index);
        self.low_links[node] = node_index;
        self.stack.push(node);
        self.on_stack[node] = true;

        for &target in &self.adjacency[node] {
            if self.indexes[target].is_none() {
                self.connect(target);
                self.low_links[node] = self.low_links[node].min(self.low_links[target]);
            } else if self.on_stack[target] {
                self.low_links[node] = self.low_links[node]
                    .min(self.indexes[target].expect("visited target must have an index"));
            }
        }

        if self.low_links[node] == node_index {
            let mut component = Vec::new();
            loop {
                let member = self.stack.pop().expect("SCC stack must contain root");
                self.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}
