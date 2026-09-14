use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    domain::{DirectoryLoadHint, DirectoryUpdateRule},
};

#[tokio::test]
async fn codespec_init_and_directory_crud_preserve_baseline() {
    let root = temp_root("codespec-directory-crud");
    fs::create_dir_all(&root).await.expect("root should create");
    fs::write(
        root.join("AGENTS.md"),
        "CodeSpec map: codespec/codespec-map.yaml\nKnowledge map: knowledge/knowledge-map.yaml\n",
    )
    .await
    .expect("agents should write");
    let service = KnowledgeMapService::new(root.clone()).for_type(RepositoryMapType::Codespec);
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    service
        .init(&context)
        .await
        .expect("codespec init should work");
    let custom = root.join("codespec/integrations");
    fs::create_dir_all(&custom)
        .await
        .expect("custom directory should create");
    fs::write(custom.join("README.md"), "# Integrations\n")
        .await
        .expect("key file should write");
    service
        .add_directory(
            &context,
            RepositoryMapDirectory {
                directory: "integrations".to_owned(),
                purpose: "Integration specifications.".to_owned(),
                content_scope: vec!["codespec/integrations/**".to_owned()],
                key_files: vec!["codespec/integrations/README.md".to_owned()],
                load_hint: DirectoryLoadHint::OnDemand,
                relations: Vec::new(),
                update_rule: DirectoryUpdateRule::Reviewed,
            },
        )
        .await
        .expect("custom directory should add");
    service
        .update_directory(
            &context,
            RepositoryMapDirectoryChange {
                directory: "integrations".to_owned(),
                purpose: None,
                content_scope: None,
                key_files: None,
                load_hint: Some(DirectoryLoadHint::TaskMatch),
                relations: None,
                update_rule: None,
            },
        )
        .await
        .expect("custom directory should update");
    assert!(
        service
            .remove_directory(&context, "design".to_owned())
            .await
            .is_err(),
        "baseline removal must fail"
    );
    service
        .remove_directory(&context, "integrations".to_owned())
        .await
        .expect("custom directory should remove");
    let validation = service
        .validate(&context)
        .await
        .expect("validate should run");
    assert!(validation.valid, "{:?}", validation.diagnostics);

    let _ = fs::remove_dir_all(root).await;
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("relay-knowledge-{name}-{nonce}"))
}
