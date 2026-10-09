use super::*;
use crate::api::InterfaceKind;

#[test]
fn feedback_handle_binds_existing_ids_without_changing_query_versions() {
    let context = RequestContext::with_ids(InterfaceKind::Cli, "request", "trace");
    let metadata = ApiMetadata::graph_only(&context, GraphVersion::ZERO);
    let value = serde_json::to_value(&metadata).unwrap();
    assert_eq!(value["feedback"]["trace_id"], value["trace_id"]);
    assert_eq!(value["feedback"]["request_id"], value["request_id"]);
    assert_eq!(value["feedback"]["schema_version"], 1);
    let mut legacy = value;
    legacy.as_object_mut().unwrap().remove("feedback");
    assert_eq!(
        serde_json::from_value::<ApiMetadata>(legacy)
            .unwrap()
            .feedback,
        None
    );
}
