use super::{extract_handler_name, extract_handler_name_from_arguments, extract_quoted_string};

#[test]
fn quoted_route_path_accepts_javascript_quote_styles() {
    assert_eq!(
        extract_quoted_string(" '/users'"),
        Some("/users".to_owned())
    );
    assert_eq!(
        extract_quoted_string("\"/health\""),
        Some("/health".to_owned())
    );
    assert_eq!(extract_quoted_string("`/items`"), Some("/items".to_owned()));
    assert_eq!(extract_quoted_string("/users/"), None);
}

#[test]
fn final_handler_skips_middleware_and_preserves_member_target() {
    assert_eq!(
        extract_handler_name("'/users', requireAuth, userController.listUsers)"),
        Some("userController.listUsers".to_owned())
    );
}

#[test]
fn callback_arrays_use_their_final_named_handler() {
    assert_eq!(
        extract_handler_name_from_arguments("[requireAuth, audit, listUsers])"),
        Some("listUsers".to_owned())
    );
}

#[test]
fn inline_callbacks_are_anonymous() {
    assert_eq!(
        extract_handler_name("'/users', async (request) => respond(request))"),
        None
    );
    assert_eq!(
        extract_handler_name("'/users', function handler(request) { return request; })"),
        None
    );
}
