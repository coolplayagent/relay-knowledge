use super::reader::digest;
use crate::{
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::research::AuthoredEvidenceBundle,
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};
use std::{path::PathBuf, sync::Arc};

pub(crate) static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) async fn fixture() -> (PathBuf, AuthoredEvidenceBundle, RelayKnowledgeService) {
    let mut nonce = [0; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!("relay-bundle-{}", digest(&nonce)));
    tokio::fs::create_dir_all(&root).await.unwrap();
    for path in ["a.txt", "b.txt"] {
        tokio::fs::write(root.join(path), b"First\nSecond\n")
            .await
            .unwrap();
    }
    let nodes: Vec<_> = (0..27).map(|i| serde_json::json!({"id":format!("concept-{i}"),"kind":"technology","label":format!("Concept {i}"),"status":"reviewed","custom":{"origin":"author"}})).collect();
    let edges: Vec<_> = (0..38).map(|i| serde_json::json!({"id":format!("relation-{i}"),"source":format!("concept-{}",i%27),"target":format!("concept-{}",(i+1)%27),"relation":"supports-analysis","evidence":[if i%2==0 {"a"} else {"b"}],"status":if i%2==0 {"user-confirmed"} else {"analysis"},"qualifiers":{"condition":format!("case-{i}")}})).collect();
    let pins: Vec<_> = ["a", "b"].into_iter().map(|id| serde_json::json!({"id":id,"source_scope":"research","artifact":{"path_base":"repository","path":format!("{id}.txt"),"sha256":digest(b"First\nSecond\n")},"span":{"start_byte":0,"end_byte":5,"start_line":1,"end_line":1},"interpretation":if id=="a" {"user_scope_confirmation"} else {"author_analysis"}})).collect();
    let bundle: AuthoredEvidenceBundle = serde_json::from_value(serde_json::json!({"schema_version":1,"id":"study","source_scope":"research","graph":{"schema_version":1,"nodes":nodes,"edges":edges,"status":"reviewed","scope":"Research claims"},"evidence":pins})).unwrap();
    save(&root, &bundle).await;
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", root.to_str().unwrap()),
            ("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap()),
            ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "local"),
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "local"),
        ],
    )
    .unwrap();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .unwrap();
    let service = RelayKnowledgeService::with_store(
        runtime,
        Arc::new(SqliteGraphStore::open_in_memory().unwrap()),
    );
    (root, bundle, service)
}

pub(super) async fn save(root: &std::path::Path, bundle: &AuthoredEvidenceBundle) {
    tokio::fs::write(
        root.join("bundle.json"),
        serde_json::to_vec(bundle).unwrap(),
    )
    .await
    .unwrap();
}
