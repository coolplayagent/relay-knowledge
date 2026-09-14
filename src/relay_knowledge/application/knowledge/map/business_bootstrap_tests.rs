use super::*;
use crate::project::KNOWLEDGE_MAP_RELATIVE_PATH;

#[tokio::test]
async fn init_creates_and_then_preserves_business_glossary_without_version_churn() {
    let root = std::env::temp_dir().join(format!(
        "relay-business-bootstrap-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).await.expect("root should create");
    fs::write(
        root.join("AGENTS.md"),
        format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}\n"),
    )
    .await
    .expect("agent contract should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let initialized = service.init(&context).await.expect("init should work");
    let glossary_path = root.join(crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH);
    let empty = fs::read(&glossary_path)
        .await
        .expect("glossary should exist");
    let glossary =
        crate::domain::BusinessGlossary::parse(&empty).expect("empty glossary should validate");
    assert!(glossary.terms.is_empty());
    assert!(String::from_utf8_lossy(&empty).contains("#     mappings:"));
    let bootstrap = initialized
        .business_bootstrap
        .as_ref()
        .expect("authoring guidance");
    assert_eq!(
        bootstrap.default_glossary_path,
        crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH
    );
    assert!(
        bootstrap
            .next_steps
            .iter()
            .any(|step| step.contains("Commit"))
    );
    let authored = "schema_version: 1\ndomains:\n  - id: sales\n    name: Sales\nterms: []\n";
    fs::write(&glossary_path, authored)
        .await
        .expect("authored glossary should write");

    let repeated = service
        .init(&context)
        .await
        .expect("repeat init should work");
    let validation = service
        .validate(&context)
        .await
        .expect("validate should run");

    assert_eq!(repeated.map_version, initialized.map_version);
    assert_eq!(fs::read_to_string(&glossary_path).await.unwrap(), authored);
    assert!(validation.valid);
    let _ = fs::remove_dir_all(root).await;
}
