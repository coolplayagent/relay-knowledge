use super::{call_display_name, inferred_caller_name_from_excerpt};

#[test]
fn nested_owner_context_disambiguates_generic_callers() {
    assert_eq!(
        call_display_name(
            Some("connection"),
            Some("repo://repo/frontend::core::stream::attachRunStream.connection"),
        )
        .as_deref(),
        Some("attachRunStream.connection")
    );
    assert_eq!(
        call_display_name(
            Some("dispatch"),
            Some("repo://repo/src::main::ServiceFactory.dispatch"),
        )
        .as_deref(),
        Some("dispatch")
    );
    assert_eq!(
        call_display_name(
            None,
            Some("repo://repo/src::main::ServiceFactory.exactOwner"),
        )
        .as_deref(),
        Some("exactOwner")
    );
    assert_eq!(
        call_display_name(
            Some("endStream"),
            Some("repo://repo/frontend::stream.endStream")
        )
        .as_deref(),
        Some("endStream")
    );
    assert_eq!(
        inferred_caller_name_from_excerpt(Some(
            "Status Table::InternalGet(const ReadOptions& options) {\nreturn Status::OK();\n}"
        ))
        .as_deref(),
        Some("InternalGet")
    );
}
