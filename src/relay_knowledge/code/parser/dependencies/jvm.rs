use super::{
    DependencySeed, SeedInput, capture_xml_text, push_seed, strip_comment,
    support::{gradle_coordinate_parts, gradle_dependency_call},
};

pub(super) fn parse_pom(content: &str, records: &mut Vec<DependencySeed>) {
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

pub(super) fn parse_gradle(content: &str, records: &mut Vec<DependencySeed>) {
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
