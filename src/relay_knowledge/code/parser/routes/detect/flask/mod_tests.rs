use super::detect_flask_routes;

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
