use serde_json::Value;

use super::{
    DependencySeed, SeedInput, line_containing_json_key, npm_group, push_seed,
    support::{npm_requirement_is_local, package_lock_entry_is_local, package_lock_package_name},
};

pub(super) fn parse_package_json(content: &str, records: &mut Vec<DependencySeed>) {
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

pub(super) fn parse_package_lock(content: &str, records: &mut Vec<DependencySeed>) {
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

fn value_as_text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}
