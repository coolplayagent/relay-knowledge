use super::*;

fn entry(directory: &str) -> RepositoryMapDirectory {
    RepositoryMapDirectory {
        directory: directory.to_owned(),
        purpose: "Govern repository knowledge.".to_owned(),
        content_scope: vec![format!("knowledge/{directory}/**")],
        key_files: vec![format!("knowledge/{directory}/README.md")],
        load_hint: DirectoryLoadHint::OnDemand,
        relations: Vec::new(),
        update_rule: DirectoryUpdateRule::Reviewed,
    }
}

#[test]
fn validates_confined_directory_contract() {
    let mut directories = RepositoryMapType::Knowledge
        .required_directories()
        .iter()
        .map(|directory| entry(directory))
        .collect::<Vec<_>>();
    directories[0].relations.push(DirectoryRelation {
        kind: DirectoryRelationKind::DependsOn,
        target: "knowledge:guides".to_owned(),
    });

    validate_directory_collection(RepositoryMapType::Knowledge, &directories, true)
        .expect("baseline should validate");
}

#[test]
fn rejects_missing_baseline_and_dependency_cycles() {
    let mut directories = RepositoryMapType::Knowledge
        .required_directories()
        .iter()
        .map(|directory| entry(directory))
        .collect::<Vec<_>>();
    directories[0].relations.push(DirectoryRelation {
        kind: DirectoryRelationKind::DependsOn,
        target: "knowledge:guides".to_owned(),
    });
    directories[1].relations.push(DirectoryRelation {
        kind: DirectoryRelationKind::DependsOn,
        target: "knowledge:domain".to_owned(),
    });
    assert!(
        validate_directory_collection(RepositoryMapType::Knowledge, &directories, true).is_err()
    );
    directories.pop();
    assert!(
        validate_directory_collection(RepositoryMapType::Knowledge, &directories, true).is_err()
    );
}

#[test]
fn rejects_paths_outside_governed_directory() {
    let mut directory = entry("domain");
    directory.key_files = vec!["docs/README.md".to_owned()];
    assert!(directory.validate(RepositoryMapType::Knowledge).is_err());
}
