use super::CODE_SNAPSHOT_FACT_VERSION;

#[test]
fn fact_version_includes_generated_and_web_route_facts() {
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("generated-files-v1"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("web-routes-v1"));
}
