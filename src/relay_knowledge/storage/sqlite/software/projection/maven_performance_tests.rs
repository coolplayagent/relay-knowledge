//! A single lifecycle POM materialization feeds reactor and build projections.
use super::{
    lifecycle,
    test_support::{create_test_schema, seed_scope},
};
use crate::domain::GraphVersion;
use rusqlite::{Connection, params};
use std::sync::atomic::{AtomicUsize, Ordering};

static POM_LOADS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn code_index_persistence_performance_suite_maven_lifecycle_loads_models_once() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_test_schema(&connection);
    super::super::schema::initialize_schema(&connection).unwrap();
    seed_scope(&connection);
    for index in 0..128 {
        let path = format!("m{index}/pom.xml");
        let content = format!(
            "<project><groupId>x</groupId><artifactId>m{index}</artifactId><version>1</version></project>"
        );
        connection.execute("INSERT INTO code_repository_files (repository_id,source_scope,file_id,path,language_id,parse_status) VALUES ('repo','scope-1',?1,?1,'xml','parsed')", [&path]).unwrap();
        connection
            .execute(
                "INSERT INTO code_repository_chunks VALUES ('repo','scope-1',?1,?1,'xml',?2,1,1)",
                params![path, content],
            )
            .unwrap();
    }
    POM_LOADS.store(0, Ordering::Relaxed);
    connection.trace(Some(|sql| {
        let sql = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        if sql.contains("SELECT repository_id, source_scope,")
            && sql.contains("path, content,")
            && sql.contains("FROM code_repository_chunks")
        {
            POM_LOADS.fetch_add(1, Ordering::Relaxed);
        }
    }));
    let transaction = connection.transaction().unwrap();
    lifecycle::delete_scope(&transaction, "scope-1").unwrap();
    let projection =
        lifecycle::refresh_projection(&transaction, "scope-1", GraphVersion::new(1)).unwrap();
    transaction.commit().unwrap();
    connection.trace(None);
    let loads = POM_LOADS.load(Ordering::Relaxed);
    eprintln!("maven_effective_model_loads={loads} modules=128");
    assert_eq!(
        loads, 1,
        "reset plus lifecycle must materialize POMs only once"
    );
    assert!(
        projection
            .build_targets
            .iter()
            .filter(|target| target.ecosystem == "maven")
            .count()
            >= 128
    );
    let modules: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM maven_reactor_modules WHERE source_scope='scope-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(modules, 128);
}
