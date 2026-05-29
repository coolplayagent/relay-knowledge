use std::path::Path;

use crate::domain::{CodeDependencyRecord, RepositoryCodeRange};

use super::super::{CodeIndexError, SnapshotBuild, stable_id};

mod cmake;
mod conan;
mod go;
mod jvm;
mod npm;
mod python;
mod rust;
mod support;
mod yaml;

pub(in crate::code::parser::dependencies) use support::{
    inline_table_bool_field, inline_table_field, python_requirement,
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
        DependencyFileKind::CargoToml => rust::parse_cargo_toml(content, &mut records),
        DependencyFileKind::CargoLock => rust::parse_cargo_lock(content, &mut records),
        DependencyFileKind::PackageJson => npm::parse_package_json(content, &mut records),
        DependencyFileKind::PackageLockJson => npm::parse_package_lock(content, &mut records),
        DependencyFileKind::GoMod => go::parse_go_mod(content, &mut records),
        DependencyFileKind::GoSum => go::parse_go_sum(content, &mut records),
        DependencyFileKind::PyprojectToml => python::parse_pyproject(content, &mut records),
        DependencyFileKind::UvLock => python::parse_uv_lock(content, &mut records),
        DependencyFileKind::RequirementsTxt => python::parse_requirements(content, &mut records),
        DependencyFileKind::PomXml => jvm::parse_pom(content, &mut records),
        DependencyFileKind::Gradle => jvm::parse_gradle(content, &mut records),
        DependencyFileKind::ConanfileTxt => conan::parse_conanfile_txt(content, &mut records),
        DependencyFileKind::ConanfilePy => conan::parse_conanfile_py(content, &mut records),
        DependencyFileKind::CMakeLists => cmake::parse_cmake_lists(content, &mut records),
        DependencyFileKind::IacYaml => yaml::parse_iac_yaml(path, content, &mut records),
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

pub(in crate::code::parser) fn dependency_manifest_is_facts_only(path: &str) -> bool {
    DependencyFileKind::from_path(path).is_some_and(DependencyFileKind::facts_only)
}

pub(in crate::code) fn dependency_manifest_overrides_default_exclusion(path: &str) -> bool {
    DependencyFileKind::from_path(path).is_some_and(DependencyFileKind::overrides_default_exclusion)
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
    UvLock,
    RequirementsTxt,
    PomXml,
    Gradle,
    ConanfileTxt,
    ConanfilePy,
    CMakeLists,
    IacYaml,
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
            "uv.lock" => Some(Self::UvLock),
            "pom.xml" => Some(Self::PomXml),
            "build.gradle" | "build.gradle.kts" => Some(Self::Gradle),
            "conanfile.txt" => Some(Self::ConanfileTxt),
            "conanfile.py" => Some(Self::ConanfilePy),
            "CMakeLists.txt" => Some(Self::CMakeLists),
            _ if yaml::iac_yaml_path(path, file_name) => Some(Self::IacYaml),
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
            Self::PyprojectToml | Self::UvLock | Self::RequirementsTxt => &["python"],
            Self::PomXml | Self::Gradle => &["java", "kotlin", "scala"],
            Self::ConanfileTxt | Self::ConanfilePy => &["c", "cpp"],
            Self::CMakeLists => &["c", "cpp"],
            Self::IacYaml => &["yaml"],
        }
    }

    fn facts_only(self) -> bool {
        matches!(self, Self::PackageLockJson | Self::UvLock)
    }

    fn overrides_default_exclusion(self) -> bool {
        matches!(self, Self::UvLock)
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
pub(in crate::code::parser::dependencies) struct DependencySeed {
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

pub(in crate::code::parser::dependencies) struct SeedInput<'a> {
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
    pub(in crate::code::parser::dependencies) fn new(
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

    pub(in crate::code::parser::dependencies) fn resolved(
        mut self,
        version: Option<String>,
    ) -> Self {
        self.resolved_version = version;
        self
    }

    pub(in crate::code::parser::dependencies) fn line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }

    pub(in crate::code::parser::dependencies) fn excerpt(
        mut self,
        excerpt: impl Into<String>,
    ) -> Self {
        self.excerpt = excerpt.into();
        self
    }
}

pub(in crate::code::parser::dependencies) fn push_seed(
    records: &mut Vec<DependencySeed>,
    input: SeedInput<'_>,
) {
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

pub(in crate::code::parser::dependencies) fn push_lock_seed(
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

pub(in crate::code::parser::dependencies) fn push_python_requirement(
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

pub(in crate::code::parser::dependencies) fn parse_assignment_dependency(
    line: &str,
) -> Option<(String, Option<String>)> {
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

pub(in crate::code::parser::dependencies) fn parse_cargo_assignment_dependency(
    line: &str,
) -> Option<(String, Option<String>)> {
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

pub(in crate::code::parser::dependencies) fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(',')
        .to_owned()
}

pub(in crate::code::parser::dependencies) fn strip_comment(line: &str, marker: char) -> &str {
    match marker {
        '/' => line.split("//").next().unwrap_or(line),
        _ => line.split(marker).next().unwrap_or(line),
    }
}

pub(in crate::code::parser::dependencies) fn npm_group(group: &str) -> &str {
    match group {
        "devDependencies" => "dev",
        "peerDependencies" => "peer",
        "optionalDependencies" => "optional",
        _ => "dependencies",
    }
}

pub(in crate::code::parser::dependencies) fn line_containing_json_key(
    content: &str,
    key: &str,
) -> Option<usize> {
    let needle = format!("\"{key}\"");
    content
        .lines()
        .position(|line| line.contains(&needle))
        .map(|index| index + 1)
}

pub(in crate::code::parser::dependencies) fn pyproject_group(section: &str) -> String {
    if let Some(rest) = section.strip_prefix("tool.poetry.group.") {
        rest.split('.').next().unwrap_or("dependencies").to_owned()
    } else if section == "tool.poetry.dev-dependencies" {
        "dev".to_owned()
    } else {
        "dependencies".to_owned()
    }
}

pub(in crate::code::parser::dependencies) fn poetry_dependency_group(
    section: &str,
) -> Option<String> {
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

pub(in crate::code::parser::dependencies) fn quoted_values(value: &str) -> Vec<&str> {
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

pub(in crate::code::parser::dependencies) fn capture_xml_text(
    line: &str,
    tag: &str,
    output: &mut Option<String>,
) {
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

pub(in crate::code::parser::dependencies) fn conan_reference(
    value: &str,
) -> Option<(String, Option<String>)> {
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
#[path = "dependencies/tests.rs"]
mod tests;
