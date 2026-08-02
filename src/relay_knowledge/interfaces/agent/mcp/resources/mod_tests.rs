use serde_json::json;

use super::{json_content, resource_descriptor, text_content};

#[test]
fn resource_descriptors_use_the_mcp_wire_field_names() {
    let descriptor = resource_descriptor(
        "relay://service/health",
        "relay_health",
        "Health",
        "application/json",
    );

    assert_eq!(descriptor["uri"], "relay://service/health");
    assert_eq!(descriptor["name"], "relay_health");
    assert_eq!(descriptor["mimeType"], "application/json");
}

#[test]
fn resource_content_preserves_uri_mime_type_and_encoded_text() {
    let Ok(json_resource) = json_content("relay://graph/summary", &json!({"count": 2})) else {
        panic!("JSON resource should render");
    };
    let text_resource = text_content(
        "relay://metrics/prometheus",
        "text/plain",
        "metric 1\n".to_owned(),
    );

    assert_eq!(json_resource["contents"][0]["text"], "{\"count\":2}");
    assert_eq!(text_resource["contents"][0]["mimeType"], "text/plain");
}
