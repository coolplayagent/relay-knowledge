use super::*;
use crate::{
    domain::GraphVersion,
    storage::sqlite::maven::model::{PomDocument, resolve_effective_model_load},
};
use rusqlite::Connection;

pub(super) fn models(poms: &[(&str, &str)]) -> Vec<super::super::model::EffectivePom> {
    resolve_effective_model_load(
        poms.iter()
            .map(|(path, content)| PomDocument {
                repository_id: "repo".into(),
                source_scope: "scope".into(),
                file_id: path.to_string(),
                path: path.to_string(),
                content: content.to_string(),
                byte_start: 0,
                byte_end: content.len() as u64,
            })
            .collect(),
    )
    .unwrap()
    .models
}

pub(super) fn fixture() -> (Vec<Module>, Vec<Edge>) {
    build::facts(&models(&[
        ("pom.xml", "<project><groupId>demo</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><modules><module>a</module><module>b</module><module>c</module></modules></project>"),
        ("a/pom.xml", "<project><groupId>demo</groupId><artifactId>a</artifactId><version>1</version><dependencies><dependency><groupId>demo</groupId><artifactId>b</artifactId><version>1</version></dependency></dependencies></project>"),
        ("b/pom.xml", "<project><groupId>demo</groupId><artifactId>b</artifactId><version>1</version><dependencies><dependency><groupId>demo</groupId><artifactId>c</artifactId><version>1</version></dependency></dependencies></project>"),
        ("c/pom.xml", "<project><groupId>demo</groupId><artifactId>c</artifactId><version>1</version></project>"),
    ]), GraphVersion::ZERO).unwrap()
}

pub(super) fn database() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    connection
        .execute_batch("CREATE TABLE code_repository_files (source_scope TEXT, path TEXT);")
        .unwrap();
    let (modules, edges) = fixture();
    persistence::persist(&connection, "scope", &modules, &edges).unwrap();
    connection
}

#[test]
fn reactor_has_one_logical_module_per_pom_and_internal_edges() {
    let (modules, edges) = fixture();
    assert_eq!(modules.len(), 4);
    assert_eq!(edges.len(), 5);
    assert!(
        edges
            .iter()
            .all(|edge| edge.relationship.resolution_state == "resolved")
    );
}
