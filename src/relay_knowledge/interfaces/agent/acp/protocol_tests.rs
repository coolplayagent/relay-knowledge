use serde_json::json;

use super::*;

#[test]
fn initialize_capability_uses_acp_wire_names() {
    let response = AcpInitializeResponse {
        meta: AcpInitializeMeta {
            relay_knowledge: AcpRelayKnowledgeCapability {
                graph_retrieval: true,
                read_only: true,
                supports_cancellation: true,
                supports_index_refresh_permission: false,
            },
        },
    };

    assert_eq!(
        serde_json::to_value(response).expect("capability should serialize"),
        json!({
            "_meta": {
                "relayKnowledge": {
                    "graphRetrieval": true,
                    "readOnly": true,
                    "supportsCancellation": true,
                    "supportsIndexRefreshPermission": false
                }
            }
        })
    );
}

#[test]
fn prompt_request_omits_absent_optional_metadata() {
    let request = AcpPromptRequest {
        prompt: "find the storage owner".to_owned(),
        request_id: None,
        meta: None,
    };

    assert_eq!(
        serde_json::to_value(request).expect("prompt should serialize"),
        json!({"prompt": "find the storage owner"})
    );
}

#[test]
fn update_constructors_preserve_protocol_state_transitions() {
    let pending = AcpSessionUpdate::pending("request-1", "accepted");
    let progress =
        AcpSessionUpdate::meta("request-1", "retrieving", json!({"sourceScope": "docs"}));
    let failed =
        AcpSessionUpdate::failed("request-1", "cancelled", AcpSessionUpdateStatus::Cancelled);

    assert_eq!(pending.kind, AcpSessionUpdateKind::ToolCallUpdate);
    assert_eq!(pending.status, AcpSessionUpdateStatus::Pending);
    assert_eq!(progress.kind, AcpSessionUpdateKind::SessionUpdate);
    assert_eq!(progress.status, AcpSessionUpdateStatus::InProgress);
    assert_eq!(progress.meta, Some(json!({"sourceScope": "docs"})));
    assert_eq!(failed.status, AcpSessionUpdateStatus::Cancelled);
}
