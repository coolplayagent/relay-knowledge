use super::{
    DependencySeed, SeedInput,
    gradle_notation::{gradle_coordinate_parts, gradle_dependency_call},
    push_seed, strip_comment,
};

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
