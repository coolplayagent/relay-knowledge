use std::collections::BTreeMap;

use super::{
    detect_flask_routes, parse_flask_decorator, parse_flask_methods_decorator,
    parse_python_router_prefix,
};

#[test]
fn rejects_decorators_without_call_arguments() {
    assert!(parse_flask_decorator("@app.route", &BTreeMap::new()).is_none());
    assert!(parse_flask_methods_decorator("@app.methods").is_none());
}

#[test]
fn accepts_blueprints_and_rejects_unrecognized_router_factories() {
    assert!(
        parse_python_router_prefix("api = Blueprint('api', __name__, url_prefix='/v1')").is_some()
    );
    assert!(parse_python_router_prefix("api = NotARouter()").is_none());
}

#[test]
fn detector_composes_blueprint_mounts_and_decorators() {
    let source = "
        from flask import Blueprint

        users = Blueprint('users', __name__, url_prefix='/users')

        @users.get('/active')
        def list_active_users():
            return []

        app.register_blueprint(users, url_prefix='/api')
    ";

    let routes = detect_flask_routes(source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users/active");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "list_active_users");
    assert_eq!(routes[0].framework, "flask");
}
