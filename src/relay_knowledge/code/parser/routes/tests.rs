use super::detect::detect_routes;

#[test]
fn detects_express_get_route() {
    let source = "app.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "listUsers");
    assert_eq!(routes[0].framework, "express");
    assert_eq!(routes[0].line, 1);
}

#[test]
fn detects_express_post_route() {
    let source = "router.post('/api/login', handleLogin);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "post");
    assert_eq!(routes[0].url, "/api/login");
    assert_eq!(routes[0].handler_name, "handleLogin");
}

#[test]
fn detects_express_put_and_delete() {
    let source = "app.put('/users/:id', update);\napp.delete('/users/:id', remove);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].http_method, "put");
    assert_eq!(routes[1].http_method, "delete");
}

#[test]
fn detects_express_patch_route() {
    let source = "app.patch('/profile', updateProfile);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "patch");
}

#[test]
fn deduplicates_express_routes() {
    let source = "app.get('/users', list);\napp.get('/users', list2);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
}

#[test]
fn skips_non_route_method_calls() {
    let source = "obj.fetch('/data', cb);\nconsole.log('hello');\n";
    let routes = detect_routes("javascript", source);
    assert!(routes.is_empty());
}

#[test]
fn detects_flask_route_with_method() {
    let source = "@app.route('/login', methods=['POST'])\ndef login():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/login");
    assert_eq!(routes[0].http_method, "post");
    assert_eq!(routes[0].handler_name, "login");
    assert_eq!(routes[0].framework, "flask");
}

#[test]
fn detects_flask_route_multiple_methods() {
    let source = "@app.route('/items', methods=['GET', 'POST'])\ndef items():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|r| r.http_method == "get"));
    assert!(routes.iter().any(|r| r.http_method == "post"));
}

#[test]
fn detects_flask_route_default_get() {
    let source = "@app.route('/status')\ndef status():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "get");
}

#[test]
fn detects_fastapi_router() {
    let source = "@router.get('/health')\ndef health():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "health");
}

#[test]
fn detects_spring_get_mapping() {
    let source = "@GetMapping(\"/users\")\npublic List<User> getUsers() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "getUsers");
    assert_eq!(routes[0].framework, "spring");
}

#[test]
fn detects_spring_post_mapping() {
    let source = "@PostMapping(\"/login\")\npublic Response login(@Body Request req) {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "post");
    assert_eq!(routes[0].handler_name, "login");
}

#[test]
fn detects_spring_class_level_request_mapping() {
    let source =
        "@RequestMapping(\"/api\")\n@GetMapping(\"/users\")\npublic List<User> getUsers() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
}

#[test]
fn detects_spring_delete_and_put_mapping() {
    let source = "@DeleteMapping(\"/users/{id}\")\npublic void deleteUser() {\n@PutMapping(\"/users/{id}\")\npublic void updateUser() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].http_method, "delete");
    assert_eq!(routes[1].http_method, "put");
}

#[test]
fn detects_spring_request_mapping_with_method() {
    let source = "@RequestMapping(value = \"/status\", method = RequestMethod.GET)\npublic String status() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "status");
}

#[test]
fn returns_empty_for_unsupported_language() {
    let routes = detect_routes("rust", "fn main() {}");
    assert!(routes.is_empty());
}

#[test]
fn skips_non_web_files() {
    let routes = detect_routes(
        "javascript",
        "const sum = (a, b) => a + b;\nconsole.log(sum(1,2));\n",
    );
    assert!(routes.is_empty());
}
