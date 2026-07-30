use super::{
    MAX_FLASK_ROUTE_DECORATOR_LINES, flask_decorator_statement, python_add_url_rule_statement,
    python_include_router_statement, python_register_blueprint_statement,
    python_router_prefix_statement,
};

#[test]
fn wrappers_only_accept_their_owned_statement_shapes() {
    let router = strings(&["router = APIRouter("]);
    let include = strings(&["app.include_router("]);
    let blueprint = strings(&["app.register_blueprint("]);
    let add_rule = strings(&["app.add_url_rule("]);

    assert!(python_router_prefix_statement(&router, 0).is_some());
    assert!(python_include_router_statement(&include, 0).is_some());
    assert!(python_register_blueprint_statement(&blueprint, 0).is_some());
    assert!(python_add_url_rule_statement(&add_rule, 0).is_some());
    assert!(python_router_prefix_statement(&include, 0).is_none());
}

#[test]
fn decorator_aggregation_respects_nested_values_and_quoted_parentheses() {
    let lines = strings(&[
        "@app.route(",
        "'/users)active',",
        "methods=(",
        "'GET',",
        "),",
        ")",
        "def users():",
    ]);

    let (statement, consumed) = flask_decorator_statement(&lines, 0);

    assert_eq!(consumed, 6);
    assert_eq!(
        statement,
        "@app.route( '/users)active', methods=( 'GET', ), )"
    );
}

#[test]
fn bounds_unclosed_decorator_aggregation() {
    let lines = (0..MAX_FLASK_ROUTE_DECORATOR_LINES + 3)
        .map(|index| {
            if index == 0 {
                "@app.route(".to_owned()
            } else {
                format!("value{index}")
            }
        })
        .collect::<Vec<_>>();

    let (_, consumed) = flask_decorator_statement(&lines, 0);

    assert_eq!(consumed, MAX_FLASK_ROUTE_DECORATOR_LINES);
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}
