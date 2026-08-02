use super::{EffectiveGoal, EffectivePlugin, EffectivePluginExecution};

fn plugin(profile: Option<&str>) -> EffectivePlugin {
    EffectivePlugin {
        artifact_id: "maven-compiler-plugin".to_owned(),
        version: None,
        executions: Vec::new(),
        line: 1,
        source_path: "pom.xml".to_owned(),
        coordinate: "org.apache.maven.plugins:maven-compiler-plugin".to_owned(),
        profile: profile.map(str::to_owned),
        inherited: true,
    }
}

#[test]
fn plugin_contract_builds_stable_prefix_scope_and_command() {
    let plugin = plugin(Some("release"));

    assert_eq!(plugin.prefix(), "compiler");
    assert_eq!(plugin.scoped_name("compile"), "profile:release:compile");
    assert_eq!(
        plugin.command("compiler:compile"),
        "mvn -Prelease compiler:compile"
    );
}

#[test]
fn execution_contract_uses_first_goal_when_phase_is_absent() {
    let execution = EffectivePluginExecution {
        id: None,
        phase: None,
        goals: vec![EffectiveGoal {
            value: "compile".to_owned(),
            line: 2,
            source_path: "pom.xml".to_owned(),
        }],
        line: 2,
        source_path: "pom.xml".to_owned(),
        inherited: true,
    };

    assert_eq!(execution.name(), "default");
    assert_eq!(
        execution.command(&plugin(None)).as_deref(),
        Some("mvn compiler:compile")
    );
}
