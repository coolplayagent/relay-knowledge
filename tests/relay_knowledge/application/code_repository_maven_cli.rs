//! Issue #390 acceptance through the original CLI, independent of Java imports.
use super::*;
use serde_json::Value;
use std::process::Command;

#[test]
fn maven_original_dependencies_command_returns_modules_declarations_and_pages() {
    let repo = FixtureRepo::create("maven-original-cli");
    repo.write("pom.xml", "<project><groupId>demo</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><modules><module>a</module><module>b</module></modules></project>");
    write_module(&repo, "a", Some("b"));
    write_module(&repo, "b", None);
    let pom = std::fs::read_to_string(repo.path.join("a/pom.xml")).unwrap().replace("</dependencies>", "<dependency><groupId>external</groupId><artifactId>logging</artifactId><version>1</version></dependency></dependencies>");
    repo.write("a/pom.xml", &pom);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Maven declaration-only dependencies"]);
    let path = repo.path.to_str().unwrap();
    cli(&repo, &["repo", "register", path, "--alias", "demo"]);
    cli(&repo, &["repo", "index", "demo", "--ref", "HEAD"]);
    let original = cli(
        &repo,
        &[
            "repo",
            "software",
            "demo",
            "--kind",
            "dependencies",
            "--ref",
            "HEAD",
        ],
    );
    assert_eq!(original["build_targets"].as_array().unwrap().len(), 3);
    assert_eq!(original["relationships"].as_array().unwrap().len(), 6);
    assert_eq!(
        original["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| edge["relationship_kind"] == "inherits_from"
                && edge["resolution_state"] == "resolved")
            .count(),
        2
    );
    assert!(original["dependency_usages"].as_array().unwrap().is_empty());
    assert!(
        original["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["target_kind"] == "artifact"
                && edge["resolution_state"] == "unresolved")
    );
    let mut cursor = None;
    let mut ids = std::collections::BTreeSet::new();
    for _ in 0..30 {
        let mut args = vec![
            "repo",
            "software",
            "demo",
            "--kind",
            "dependencies",
            "--ref",
            "HEAD",
            "--limit",
            "1",
        ];
        if let Some(value) = cursor.as_deref() {
            args.extend(["--cursor", value]);
        }
        let page = cli(&repo, &args);
        let mut count = 0;
        for (field, key) in [
            ("build_targets", "target_id"),
            ("relationships", "relationship_id"),
            ("components", "component_id"),
            ("dependency_usages", "usage_id"),
        ] {
            for fact in page[field].as_array().unwrap() {
                count += 1;
                assert!(ids.insert(fact[key].as_str().unwrap().to_owned()));
            }
        }
        assert_eq!(count, 1);
        cursor = page["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    let expected = [
        "build_targets",
        "relationships",
        "components",
        "dependency_usages",
    ]
    .iter()
    .map(|key| original[key].as_array().unwrap().len())
    .sum::<usize>();
    assert_eq!(ids.len(), expected);
}

fn cli(repo: &FixtureRepo, args: &[&str]) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_relay-knowledge"));
    // Isolated CLI integration-test process; do not inherit developer runtime configuration.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("RELAY_KNOWLEDGE_") {
            command.env_remove(name);
        }
    }
    let output = command
        .env(
            "RELAY_KNOWLEDGE_HOME",
            repo.path.with_extension("cli-runtime"),
        )
        .args(args)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
