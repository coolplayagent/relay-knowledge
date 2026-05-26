use std::{collections::HashSet, path::Path};

use serde_json::Value;

use crate::domain::{CodeDependencyRecord, RepositoryCodeRange};

use super::super::{CodeIndexError, SnapshotBuild, stable_id};

#[path = "dependencies_support.rs"]
mod support;

use support::{
    cargo_lock_source_is_external, gradle_coordinate_parts, gradle_dependency_call,
    inline_table_bool_field, inline_table_field, npm_requirement_is_local,
    package_lock_entry_is_local, package_lock_package_name, python_requirement,
    requirements_dependency_line,
};

pub(super) fn collect_dependencies(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    content: &str,
) -> Result<Vec<CodeDependencyRecord>, CodeIndexError> {
    let Some(kind) = DependencyFileKind::from_path(path) else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    match kind {
        DependencyFileKind::CargoToml => parse_cargo_toml(content, &mut records),
        DependencyFileKind::CargoLock => parse_cargo_lock(content, &mut records),
        DependencyFileKind::PackageJson => parse_package_json(content, &mut records),
        DependencyFileKind::PackageLockJson => parse_package_lock(content, &mut records),
        DependencyFileKind::GoMod => parse_go_mod(content, &mut records),
        DependencyFileKind::GoSum => parse_go_sum(content, &mut records),
        DependencyFileKind::PyprojectToml => parse_pyproject(content, &mut records),
        DependencyFileKind::RequirementsTxt => parse_requirements(content, &mut records),
        DependencyFileKind::PomXml => parse_pom(content, &mut records),
        DependencyFileKind::Gradle => parse_gradle(content, &mut records),
        DependencyFileKind::ConanfileTxt => parse_conanfile_txt(content, &mut records),
        DependencyFileKind::ConanfilePy => parse_conanfile_py(content, &mut records),
    }
    Ok(records
        .into_iter()
        .enumerate()
        .flat_map(|(seed_index, seed)| {
            dependency_record_language_ids(build, kind, seed.language_id)
                .into_iter()
                .enumerate()
                .map(move |(language_index, language_id)| {
                    let index = seed_index * kind.language_ids().len() + language_index;
                    record_from_seed(build, path, file_id, index, language_id, seed.clone())
                })
        })
        .collect())
}

pub(in crate::code) fn dependency_manifest_language_ids(
    path: &str,
) -> Option<&'static [&'static str]> {
    DependencyFileKind::from_path(path).map(DependencyFileKind::language_ids)
}

#[derive(Clone, Copy)]
enum DependencyFileKind {
    CargoToml,
    CargoLock,
    PackageJson,
    PackageLockJson,
    GoMod,
    GoSum,
    PyprojectToml,
    RequirementsTxt,
    PomXml,
    Gradle,
    ConanfileTxt,
    ConanfilePy,
}

impl DependencyFileKind {
    fn from_path(path: &str) -> Option<Self> {
        let file_name = Path::new(path).file_name()?.to_str()?;
        match file_name {
            "Cargo.toml" => Some(Self::CargoToml),
            "Cargo.lock" => Some(Self::CargoLock),
            "package.json" => Some(Self::PackageJson),
            "package-lock.json" => Some(Self::PackageLockJson),
            "go.mod" => Some(Self::GoMod),
            "go.sum" => Some(Self::GoSum),
            "pyproject.toml" => Some(Self::PyprojectToml),
            "pom.xml" => Some(Self::PomXml),
            "build.gradle" | "build.gradle.kts" => Some(Self::Gradle),
            "conanfile.txt" => Some(Self::ConanfileTxt),
            "conanfile.py" => Some(Self::ConanfilePy),
            _ if python_requirements_path(path, file_name) => Some(Self::RequirementsTxt),
            _ => None,
        }
    }

    fn language_ids(self) -> &'static [&'static str] {
        match self {
            Self::CargoToml | Self::CargoLock => &["rust"],
            Self::PackageJson | Self::PackageLockJson => {
                &["javascript", "jsx", "typescript", "tsx"]
            }
            Self::GoMod | Self::GoSum => &["go"],
            Self::PyprojectToml | Self::RequirementsTxt => &["python"],
            Self::PomXml | Self::Gradle => &["java", "kotlin", "scala"],
            Self::ConanfileTxt | Self::ConanfilePy => &["c", "cpp"],
        }
    }
}

fn dependency_record_language_ids(
    build: &SnapshotBuild,
    kind: DependencyFileKind,
    default_language_id: &'static str,
) -> Vec<&'static str> {
    let compatible = kind.language_ids();
    if build.language_filters().is_empty() {
        return compatible.to_vec();
    }

    let mut selected = Vec::new();
    for language_id in compatible {
        if build
            .language_filters()
            .iter()
            .any(|filter| filter.as_str() == *language_id)
        {
            selected.push(*language_id);
        }
    }

    if selected.is_empty() {
        vec![default_language_id]
    } else {
        selected
    }
}

fn python_requirements_path(path: &str, file_name: &str) -> bool {
    file_name.ends_with(".txt")
        && (file_name.starts_with("requirements")
            || file_name.starts_with("constraints")
            || path.split('/').any(|segment| segment == "requirements"))
}

#[derive(Clone)]
struct DependencySeed {
    ecosystem: &'static str,
    language_id: &'static str,
    package_name: String,
    requirement: Option<String>,
    resolved_version: Option<String>,
    dependency_group: String,
    source_kind: &'static str,
    is_lockfile: bool,
    line: usize,
    excerpt: String,
}

fn record_from_seed(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    index: usize,
    language_id: &str,
    seed: DependencySeed,
) -> CodeDependencyRecord {
    let line = seed.line.max(1);
    CodeDependencyRecord {
        repository_id: build.repository_id.clone(),
        source_scope: build.source_scope.clone(),
        dependency_id: stable_id(
            "dependency",
            [
                build.repository_id.as_str(),
                build.source_scope.as_str(),
                path,
                language_id,
                seed.ecosystem,
                seed.package_name.as_str(),
                seed.dependency_group.as_str(),
                seed.source_kind,
                &line.to_string(),
                &index.to_string(),
            ],
        ),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        ecosystem: seed.ecosystem.to_owned(),
        package_name: seed.package_name,
        requirement: seed.requirement,
        resolved_version: seed.resolved_version,
        dependency_group: seed.dependency_group,
        source_kind: seed.source_kind.to_owned(),
        is_lockfile: seed.is_lockfile,
        line_range: RepositoryCodeRange {
            start: line as u32,
            end: line as u32,
        },
        excerpt: seed.excerpt,
    }
}

fn parse_cargo_toml(content: &str, records: &mut Vec<DependencySeed>) {
    let mut group = None::<String>;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            group = cargo_group(trimmed.trim_matches(['[', ']']));
            continue;
        }
        let Some(group) = group.as_deref() else {
            continue;
        };
        let Some((name, requirement)) = parse_cargo_assignment_dependency(trimmed) else {
            continue;
        };
        push_seed(
            records,
            SeedInput::new(
                "cargo",
                "rust",
                name,
                requirement,
                group,
                "Cargo.toml",
                false,
            )
            .line(index + 1)
            .excerpt(trimmed),
        );
    }
}

fn cargo_group(section: &str) -> Option<String> {
    if section == "dependencies" || section.ends_with(".dependencies") {
        Some("dependencies".to_owned())
    } else if section == "dev-dependencies" || section.ends_with(".dev-dependencies") {
        Some("dev".to_owned())
    } else if section == "build-dependencies" || section.ends_with(".build-dependencies") {
        Some("build".to_owned())
    } else {
        None
    }
}

fn parse_cargo_lock(content: &str, records: &mut Vec<DependencySeed>) {
    let mut in_package = false;
    let mut name = None::<(String, usize)>;
    let mut version = None::<String>;
    let mut source = None::<String>;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            push_cargo_lock_seed(records, name.take(), version.take(), source.take());
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("name = ") {
            name = Some((unquote(value), index + 1));
        } else if let Some(value) = trimmed.strip_prefix("version = ") {
            version = Some(unquote(value));
        } else if let Some(value) = trimmed.strip_prefix("source = ") {
            source = Some(unquote(value));
        }
    }
    push_cargo_lock_seed(records, name, version, source);
}

fn push_cargo_lock_seed(
    records: &mut Vec<DependencySeed>,
    name: Option<(String, usize)>,
    version: Option<String>,
    source: Option<String>,
) {
    if !cargo_lock_source_is_external(source.as_deref()) {
        return;
    }
    push_lock_seed(records, "cargo", "rust", "Cargo.lock", name, version);
}

fn parse_package_json(content: &str, records: &mut Vec<DependencySeed>) {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return;
    };
    for group in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        let Some(dependencies) = value.get(group).and_then(Value::as_object) else {
            continue;
        };
        for (name, requirement) in dependencies {
            let requirement = requirement.as_str().map(str::to_owned);
            if requirement.as_deref().is_some_and(npm_requirement_is_local) {
                continue;
            }
            let line = line_containing_json_key(content, name).unwrap_or(1);
            push_seed(
                records,
                SeedInput::new(
                    "npm",
                    "javascript",
                    name.to_owned(),
                    requirement,
                    npm_group(group),
                    "package.json",
                    false,
                )
                .line(line)
                .excerpt(format!("{name} {}", value_as_text(dependencies.get(name)))),
            );
        }
    }
}

fn parse_package_lock(content: &str, records: &mut Vec<DependencySeed>) {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return;
    };
    if let Some(packages) = value.get("packages").and_then(Value::as_object) {
        for (path, package) in packages {
            if package_lock_entry_is_local(package) {
                continue;
            }
            let Some(name) = package_lock_package_name(path, package) else {
                continue;
            };
            let version = package
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let line = line_containing_json_key(content, &name).unwrap_or(1);
            push_seed(
                records,
                SeedInput::new(
                    "npm",
                    "javascript",
                    name.clone(),
                    None,
                    "locked",
                    "package-lock.json",
                    true,
                )
                .resolved(version)
                .line(line)
                .excerpt(name),
            );
        }
        return;
    }
    if let Some(dependencies) = value.get("dependencies").and_then(Value::as_object) {
        collect_package_lock_v1_dependencies(content, dependencies, records);
    }
}

fn collect_package_lock_v1_dependencies(
    content: &str,
    dependencies: &serde_json::Map<String, Value>,
    records: &mut Vec<DependencySeed>,
) {
    for (name, package) in dependencies {
        let version = package
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let line = line_containing_json_key(content, name).unwrap_or(1);
        if !version.as_deref().is_some_and(npm_requirement_is_local) {
            push_seed(
                records,
                SeedInput::new(
                    "npm",
                    "javascript",
                    name.to_owned(),
                    None,
                    "locked",
                    "package-lock.json",
                    true,
                )
                .resolved(version)
                .line(line)
                .excerpt(name.as_str()),
            )
        }
        if let Some(nested) = package.get("dependencies").and_then(Value::as_object) {
            collect_package_lock_v1_dependencies(content, nested, records);
        }
    }
}

fn parse_go_mod(content: &str, records: &mut Vec<DependencySeed>) {
    let mut in_require_block = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '/').trim();
        if trimmed == "require (" {
            in_require_block = true;
            continue;
        }
        if in_require_block && trimmed == ")" {
            in_require_block = false;
            continue;
        }
        let require_line = if let Some(rest) = trimmed.strip_prefix("require ") {
            rest
        } else if in_require_block {
            trimmed
        } else {
            continue;
        };
        let mut parts = require_line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        let version = parts.next().map(str::to_owned);
        if !name.contains('.') {
            continue;
        }
        push_seed(
            records,
            SeedInput::new(
                "go",
                "go",
                name.to_owned(),
                version,
                "require",
                "go.mod",
                false,
            )
            .line(index + 1)
            .excerpt(require_line),
        );
    }
}

fn parse_go_sum(content: &str, records: &mut Vec<DependencySeed>) {
    let mut seen = HashSet::new();
    for (index, line) in content.lines().enumerate() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(version)) = (parts.next(), parts.next()) else {
            continue;
        };
        let version = version.strip_suffix("/go.mod").unwrap_or(version);
        if !name.contains('.') {
            continue;
        }
        if !seen.insert((name.to_owned(), version.to_owned())) {
            continue;
        }
        push_seed(
            records,
            SeedInput::new("go", "go", name.to_owned(), None, "locked", "go.sum", true)
                .resolved(Some(version.to_owned()))
                .line(index + 1)
                .excerpt(line.trim()),
        );
    }
}

fn parse_pyproject(content: &str, records: &mut Vec<DependencySeed>) {
    let mut section = String::new();
    let mut group = "dependencies".to_owned();
    let mut poetry_group = None::<String>;
    let mut in_array = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.trim_matches(['[', ']']).to_owned();
            group = pyproject_group(&section);
            poetry_group = poetry_dependency_group(&section);
            in_array = false;
            continue;
        }
        if section == "project" && trimmed.starts_with("dependencies") {
            in_array = trimmed.contains('[') && !trimmed.contains(']');
            for item in quoted_values(trimmed) {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            continue;
        }
        if section.starts_with("project.optional-dependencies") && trimmed.contains('=') {
            group = trimmed
                .split_once('=')
                .map(|(left, _)| left.trim().to_owned())
                .unwrap_or_else(|| "optional".to_owned());
            in_array = trimmed.contains('[') && !trimmed.contains(']');
            for item in quoted_values(trimmed) {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            continue;
        }
        if in_array {
            for item in quoted_values(trimmed) {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            if trimmed.contains(']') {
                in_array = false;
            }
        } else if let Some(group) = poetry_group.as_deref() {
            let Some((name, requirement)) = parse_assignment_dependency(trimmed) else {
                continue;
            };
            if name != "python" {
                push_seed(
                    records,
                    SeedInput::new(
                        "python",
                        "python",
                        name,
                        requirement,
                        group,
                        "pyproject.toml",
                        false,
                    )
                    .line(index + 1)
                    .excerpt(trimmed),
                );
            }
        }
    }
}

fn parse_requirements(content: &str, records: &mut Vec<DependencySeed>) {
    for (index, line) in content.lines().enumerate() {
        let Some(requirement) = requirements_dependency_line(line) else {
            continue;
        };
        push_python_requirement(
            records,
            requirement,
            "requirements",
            "requirements.txt",
            index + 1,
        );
    }
}

fn parse_pom(content: &str, records: &mut Vec<DependencySeed>) {
    let mut in_dependency = false;
    let mut in_dependency_management = false;
    let mut start_line = 1usize;
    let mut group_id = None::<String>;
    let mut artifact_id = None::<String>;
    let mut version = None::<String>;
    let mut scope = None::<String>;
    let mut dep_type = None::<String>;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let closes_dependency_management = trimmed.contains("</dependencyManagement>");
        if trimmed.contains("<dependencyManagement>") {
            in_dependency_management = true;
        }
        if trimmed.contains("<dependency>") {
            in_dependency = true;
            start_line = index + 1;
            group_id = None;
            artifact_id = None;
            version = None;
            scope = None;
            dep_type = None;
        }
        if !in_dependency {
            if closes_dependency_management {
                in_dependency_management = false;
            }
            continue;
        }
        capture_xml_text(trimmed, "groupId", &mut group_id);
        capture_xml_text(trimmed, "artifactId", &mut artifact_id);
        capture_xml_text(trimmed, "version", &mut version);
        capture_xml_text(trimmed, "scope", &mut scope);
        capture_xml_text(trimmed, "type", &mut dep_type);
        if !trimmed.contains("</dependency>") {
            continue;
        }
        if let (Some(group_id), Some(artifact_id)) = (group_id.take(), artifact_id.take()) {
            let bom = in_dependency_management
                && dep_type.as_deref() == Some("pom")
                && scope.as_deref() == Some("import");
            if in_dependency_management && !bom {
                in_dependency = false;
                if closes_dependency_management {
                    in_dependency_management = false;
                }
                continue;
            }
            let package_name = format!("{group_id}:{artifact_id}");
            let dependency_group = if bom {
                "bom".to_owned()
            } else {
                scope.clone().unwrap_or_else(|| "compile".to_owned())
            };
            push_seed(
                records,
                SeedInput::new(
                    "maven",
                    "java",
                    package_name,
                    version.take(),
                    dependency_group.as_str(),
                    "pom.xml",
                    false,
                )
                .line(start_line)
                .excerpt(format!("{group_id}:{artifact_id}")),
            );
        }
        in_dependency = false;
        if closes_dependency_management {
            in_dependency_management = false;
        }
    }
}

fn parse_gradle(content: &str, records: &mut Vec<DependencySeed>) {
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '/').trim();
        let Some((configuration, dependency)) = gradle_dependency_call(trimmed) else {
            continue;
        };
        let (group, artifact, version) = gradle_coordinate_parts(&dependency);
        let Some(group) = group else {
            continue;
        };
        let Some(artifact) = artifact else {
            continue;
        };
        let is_bom = trimmed.contains("platform(") || trimmed.contains("enforcedPlatform(");
        let dependency_group = if is_bom { "bom" } else { &configuration };
        push_seed(
            records,
            SeedInput::new(
                "gradle",
                "java",
                format!("{group}:{artifact}"),
                version,
                dependency_group,
                "build.gradle",
                false,
            )
            .line(index + 1)
            .excerpt(trimmed),
        );
    }
}

fn parse_conanfile_txt(content: &str, records: &mut Vec<DependencySeed>) {
    let mut section = String::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.trim_matches(['[', ']']).to_owned();
            continue;
        }
        if !matches!(
            section.as_str(),
            "requires" | "tool_requires" | "build_requires"
        ) {
            continue;
        }
        if let Some((name, version)) = conan_reference(trimmed) {
            push_seed(
                records,
                SeedInput::new(
                    "conan",
                    "cpp",
                    name,
                    version,
                    section.as_str(),
                    "conanfile.txt",
                    false,
                )
                .line(index + 1)
                .excerpt(trimmed),
            );
        }
    }
}

fn parse_conanfile_py(content: &str, records: &mut Vec<DependencySeed>) {
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        let group = if trimmed.contains("build_requires(") || trimmed.contains("tool_requires(") {
            Some("build_requires")
        } else if trimmed.contains("requires(") || trimmed.starts_with("requires =") {
            Some("requires")
        } else {
            None
        };
        let Some(group) = group else {
            continue;
        };
        for quoted in quoted_values(trimmed) {
            if let Some((name, version)) = conan_reference(quoted) {
                push_seed(
                    records,
                    SeedInput::new("conan", "cpp", name, version, group, "conanfile.py", false)
                        .line(index + 1)
                        .excerpt(trimmed),
                );
            }
        }
    }
}

struct SeedInput<'a> {
    ecosystem: &'static str,
    language_id: &'static str,
    package_name: String,
    requirement: Option<String>,
    resolved_version: Option<String>,
    dependency_group: String,
    source_kind: &'static str,
    is_lockfile: bool,
    line: usize,
    excerpt: String,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> SeedInput<'a> {
    fn new(
        ecosystem: &'static str,
        language_id: &'static str,
        package_name: String,
        requirement: Option<String>,
        dependency_group: impl Into<String>,
        source_kind: &'static str,
        is_lockfile: bool,
    ) -> Self {
        Self {
            ecosystem,
            language_id,
            package_name,
            requirement,
            resolved_version: None,
            dependency_group: dependency_group.into(),
            source_kind,
            is_lockfile,
            line: 1,
            excerpt: String::new(),
            _marker: std::marker::PhantomData,
        }
    }

    fn resolved(mut self, version: Option<String>) -> Self {
        self.resolved_version = version;
        self
    }

    fn line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }

    fn excerpt(mut self, excerpt: impl Into<String>) -> Self {
        self.excerpt = excerpt.into();
        self
    }
}

fn push_seed(records: &mut Vec<DependencySeed>, input: SeedInput<'_>) {
    if input.package_name.trim().is_empty() {
        return;
    }
    records.push(DependencySeed {
        ecosystem: input.ecosystem,
        language_id: input.language_id,
        package_name: input.package_name,
        requirement: input.requirement,
        resolved_version: input.resolved_version,
        dependency_group: input.dependency_group,
        source_kind: input.source_kind,
        is_lockfile: input.is_lockfile,
        line: input.line,
        excerpt: input.excerpt,
    });
}

fn push_lock_seed(
    records: &mut Vec<DependencySeed>,
    ecosystem: &'static str,
    language_id: &'static str,
    source_kind: &'static str,
    name: Option<(String, usize)>,
    version: Option<String>,
) {
    let Some((name, line)) = name else {
        return;
    };
    push_seed(
        records,
        SeedInput::new(
            ecosystem,
            language_id,
            name.clone(),
            None,
            "locked",
            source_kind,
            true,
        )
        .resolved(version)
        .line(line)
        .excerpt(name),
    );
}

fn push_python_requirement(
    records: &mut Vec<DependencySeed>,
    value: &str,
    group: &str,
    source_kind: &'static str,
    line: usize,
) {
    let Some((name, requirement)) = python_requirement(value) else {
        return;
    };
    push_seed(
        records,
        SeedInput::new(
            "python",
            "python",
            name,
            requirement,
            group,
            source_kind,
            false,
        )
        .line(line)
        .excerpt(value),
    );
}

fn parse_assignment_dependency(line: &str) -> Option<(String, Option<String>)> {
    let (name, value) = line.split_once('=')?;
    let name = name.trim().trim_matches('"').trim_matches('\'').to_owned();
    if name.is_empty() {
        return None;
    }
    let value = value.trim();
    let requirement = if value.starts_with('{') {
        if inline_table_field(value, "path").is_some() {
            return None;
        }
        inline_table_field(value, "version").or_else(|| inline_table_field(value, "rev"))
    } else {
        Some(unquote(value.trim_end_matches(',')))
    };
    Some((name, requirement.filter(|value| !value.is_empty())))
}

fn parse_cargo_assignment_dependency(line: &str) -> Option<(String, Option<String>)> {
    let (name, value) = line.split_once('=')?;
    let name = name.trim().trim_matches('"').trim_matches('\'').to_owned();
    if name.is_empty() {
        return None;
    }
    let value = value.trim();
    if !value.starts_with('{') {
        return Some((name, Some(unquote(value.trim_end_matches(',')))));
    }
    if cargo_inline_dependency_is_local(value) {
        return None;
    }
    let package_name = inline_table_field(value, "package").unwrap_or(name);
    let requirement = inline_table_field(value, "version")
        .or_else(|| inline_table_field(value, "rev"))
        .or_else(|| inline_table_field(value, "tag"))
        .or_else(|| inline_table_field(value, "branch"))
        .or_else(|| inline_table_field(value, "git"));
    Some((package_name, requirement.filter(|value| !value.is_empty())))
}

fn cargo_inline_dependency_is_local(value: &str) -> bool {
    inline_table_field(value, "path").is_some()
        || inline_table_bool_field(value, "workspace") == Some(true)
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(',')
        .to_owned()
}

fn strip_comment(line: &str, marker: char) -> &str {
    match marker {
        '/' => line.split("//").next().unwrap_or(line),
        _ => line.split(marker).next().unwrap_or(line),
    }
}

fn npm_group(group: &str) -> &str {
    match group {
        "devDependencies" => "dev",
        "peerDependencies" => "peer",
        "optionalDependencies" => "optional",
        _ => "dependencies",
    }
}

fn value_as_text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn line_containing_json_key(content: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    content
        .lines()
        .position(|line| line.contains(&needle))
        .map(|index| index + 1)
}

fn pyproject_group(section: &str) -> String {
    if let Some(rest) = section.strip_prefix("tool.poetry.group.") {
        rest.split('.').next().unwrap_or("dependencies").to_owned()
    } else if section == "tool.poetry.dev-dependencies" {
        "dev".to_owned()
    } else {
        "dependencies".to_owned()
    }
}

fn poetry_dependency_group(section: &str) -> Option<String> {
    if section == "tool.poetry.dependencies" {
        Some("dependencies".to_owned())
    } else if section == "tool.poetry.dev-dependencies" {
        Some("dev".to_owned())
    } else if let Some(rest) = section.strip_prefix("tool.poetry.group.") {
        let (group, suffix) = rest.split_once('.')?;
        (suffix == "dependencies" && !group.is_empty()).then(|| group.to_owned())
    } else {
        None
    }
}

fn quoted_values(value: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut start = None::<usize>;
    let mut quote = '\0';
    for (index, character) in value.char_indices() {
        if start.is_none() && matches!(character, '"' | '\'') {
            start = Some(index + character.len_utf8());
            quote = character;
        } else if start.is_some() && character == quote {
            let value_start = start.take().unwrap_or_default();
            values.push(&value[value_start..index]);
        }
    }
    values
}

fn capture_xml_text(line: &str, tag: &str, output: &mut Option<String>) {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(after_open) = line.split_once(&open).map(|(_, right)| right) else {
        return;
    };
    let Some(value) = after_open.split_once(&close).map(|(left, _)| left.trim()) else {
        return;
    };
    if !value.is_empty() {
        *output = Some(value.to_owned());
    }
}

fn conan_reference(value: &str) -> Option<(String, Option<String>)> {
    let value = value.trim().trim_matches(',').trim();
    let (name, rest) = value.split_once('/')?;
    let version = rest
        .split('@')
        .next()
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_owned);
    Some((name.trim().to_owned(), version))
}

#[cfg(test)]
#[path = "dependencies_tests.rs"]
mod tests;
