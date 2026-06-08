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
fn skips_express_like_client_get_calls() {
    let source = "axios.get('/users');\ncache.get('/health');\nclient.post('/events');\n";
    let routes = detect_routes("typescript", source);
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
fn detects_flask_route_with_tuple_methods() {
    let source = "@app.route('/login', methods=('POST',))\ndef login():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/login");
    assert_eq!(routes[0].http_method, "post");
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

#[test]
fn detects_express_anonymous_handler() {
    let source = "app.get('/health', (req, res) => {\n  res.json({ ok: true });\n});\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "anonymous");
    assert_eq!(routes[0].line, 1);
}

#[test]
fn detects_express_async_inline_handler_as_anonymous() {
    let source = "app.get('/health', async (req, res) => {\n  res.json({ ok: true });\n});\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].handler_name, "anonymous");
}

#[test]
fn detects_flask_shorthand_get_method() {
    let source = "@app.get('/ping')\ndef ping():\n    return 'pong'\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/ping");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "ping");
    assert_eq!(routes[0].framework, "flask");
    assert_eq!(routes[0].line, 2);
}

#[test]
fn detects_flask_async_route_handler() {
    let source = "@app.get('/async')\nasync def async_handler():\n    return 'ok'\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/async");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "async_handler");
}

#[test]
fn detects_flask_stacked_route_decorators() {
    let source = "@app.get('/items')\n@app.post('/items')\ndef items():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.http_method == "get"));
    assert!(routes.iter().any(|route| route.http_method == "post"));
    assert!(routes.iter().all(|route| route.url == "/items"));
}

#[test]
fn detects_fastapi_router_prefix() {
    let source =
        "router = APIRouter(prefix='/api')\n@router.get('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].handler_name, "users");
    assert_eq!(routes[0].framework, "fastapi");
}

#[test]
fn detects_fastapi_api_route_decorator_methods() {
    let source = "@router.api_route('/items', methods=['GET', 'POST'])\ndef items():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/items" && route.http_method == "get")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/items" && route.http_method == "post")
    );
}

#[test]
fn detects_typed_fastapi_router_prefix() {
    let source = "router: APIRouter = APIRouter(prefix='/api')\n@router.get('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].framework, "fastapi");
}

#[test]
fn merges_fastapi_include_router_prefix() {
    let source = "router = APIRouter()\napp.include_router(router, prefix='/api')\n@router.get('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].framework, "fastapi");
}

#[test]
fn detects_flask_shorthand_post_method() {
    let source = "@app.post('/items')\ndef create_item():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/items");
    assert_eq!(routes[0].http_method, "post");
    assert_eq!(routes[0].handler_name, "create_item");
}

#[test]
fn detects_flask_shorthand_put_method() {
    let source = "@app.put('/items/1')\ndef update_item():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "put");
    assert_eq!(routes[0].handler_name, "update_item");
}

#[test]
fn detects_flask_shorthand_delete_method() {
    let source = "@app.delete('/items/1')\ndef delete_item():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "delete");
}

#[test]
fn detects_flask_shorthand_patch_method() {
    let source = "@app.patch('/items/1')\ndef patch_item():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "patch");
}

#[test]
fn detects_flask_methods_decorator() {
    let source = "@app.route('/api/data')\n@app.methods('PUT')\ndef data():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "put");
}

#[test]
fn skips_flask_non_route_decorator() {
    let source = "@app.template_filter('reverse')\ndef reverse_filter(s):\n    return s[::-1]\n";
    let routes = detect_routes("python", source);
    assert!(routes.is_empty());
}

#[test]
fn skips_flask_decorator_without_matching_function() {
    let source = "@app.route('/orphan')\n# no function follows\nsomething_else()\n";
    let routes = detect_routes("python", source);
    assert!(routes.is_empty());
}

#[test]
fn detects_flask_route_with_head_option_methods() {
    let source = "@app.route('/check', methods=['HEAD', 'OPTIONS'])\ndef check():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|r| r.http_method == "head"));
    assert!(routes.iter().any(|r| r.http_method == "options"));
}

#[test]
fn detects_flask_router_delete_shorthand() {
    let source = "@router.delete('/items/1')\ndef delete_item():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "delete");
}

#[test]
fn detects_spring_patch_mapping() {
    let source = "@PatchMapping(\"/profile\")\npublic void patchProfile() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "patch");
    assert_eq!(routes[0].handler_name, "patchProfile");
}

#[test]
fn detects_spring_value_attribute() {
    let source = "@GetMapping(value = \"/api/v2/users\")\npublic List<User> listUsers() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/v2/users");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn detects_spring_path_attribute() {
    let source = "@PostMapping(path = \"/orders\")\npublic Order createOrder() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/orders");
    assert_eq!(routes[0].http_method, "post");
}

#[test]
fn skips_spring_non_mapping_annotation() {
    let source = "@Autowired\nprivate UserService userService;\n";
    let routes = detect_routes("java", source);
    assert!(routes.is_empty());
}

#[test]
fn detects_spring_empty_mapping_url() {
    let source = "@GetMapping\npublic String home() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/");
}

#[test]
fn detects_spring_request_mapping_post_method() {
    let source = "@RequestMapping(value = \"/submit\", method = RequestMethod.POST)\npublic String submit() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "post");
}

#[test]
fn skips_java_non_method_lines_between_annotations() {
    let source = "@GetMapping(\"/a\")\nsome random text\n";
    let routes = detect_routes("java", source);
    assert!(routes.is_empty());
}

#[test]
fn detects_spring_method_line_between_annotations() {
    let source = "@GetMapping(\"/a\")\npublic void handlerA() {\n@GetMapping(\"/b\")\npublic void handlerB() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 2);
}

#[test]
fn detects_spring_class_prefix_with_empty_suffix() {
    let source = "@RequestMapping(\"/api\")\n@GetMapping\npublic String root() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api");
}

#[test]
fn detects_flask_multiple_routes_in_sequence() {
    let source = "@app.get('/a')\ndef a():\n    pass\n\n@app.post('/b')\ndef b():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].url, "/a");
    assert_eq!(routes[1].url, "/b");
}

#[test]
fn skips_python_non_def_after_decorator() {
    let source = "@app.route('/skip')\nclass Something:\n    pass\n";
    let routes = detect_routes("python", source);
    assert!(routes.is_empty());
}

#[test]
fn detects_flask_method_decorator_with_dot_methods() {
    let source = "@app.route('/data')\n@app.methods('POST')\ndef data():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "post");
}

#[test]
fn detects_flask_router_shorthand() {
    let source = "@router.put('/items')\ndef update():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "put");
    assert_eq!(routes[0].handler_name, "update");
}

#[test]
fn detects_flask_route_with_methods_kwarg() {
    let source = "@app.route('/api', methods=['GET', 'POST'])\ndef api():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|r| r.http_method == "get"));
    assert!(routes.iter().any(|r| r.http_method == "post"));
}

#[test]
fn detects_flask_with_backslash_in_url() {
    let source = "@app.route('/path\\\\/sub')\ndef handler():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
}

#[test]
fn detects_flask_empty_methods_list_defaults_to_get() {
    let source = "@app.route('/default')\ndef handler():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "get");
}

#[test]
fn detects_spring_request_mapping_with_method_attribute() {
    let source = "@RequestMapping(value = \"/data\", method = RequestMethod.DELETE)\npublic String deleteData() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].http_method, "delete");
    assert_eq!(routes[0].url, "/data");
}

#[test]
fn detects_spring_class_prefix_with_method_attribute() {
    let source = "@RequestMapping(value = \"/api\", method = RequestMethod.GET)\npublic class UserController {\n@GetMapping(\"/users\")\npublic List<User> users() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].http_method, "get");
}

#[test]
fn spring_class_level_method_constrains_methodless_request_mapping() {
    let source = "@RequestMapping(path = \"/api\", method = RequestMethod.POST)\npublic class LoginController {\n@RequestMapping(\"/login\")\npublic String login() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/login");
    assert_eq!(routes[0].http_method, "post");
}

#[test]
fn detects_spring_static_imported_request_methods() {
    let source =
        "@RequestMapping(value = \"/submit\", method = {GET, POST})\npublic String submit() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.http_method == "get"));
    assert!(routes.iter().any(|route| route.http_method == "post"));
    assert!(routes.iter().all(|route| route.url == "/submit"));
}

#[test]
fn expands_spring_multiple_class_prefixes() {
    let source = "@RequestMapping({\"/api\", \"/v1\"})\npublic class UserController {\n@GetMapping(\"/users\")\npublic List<User> users() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.url == "/api/users"));
    assert!(routes.iter().any(|route| route.url == "/v1/users"));
}

#[test]
fn resets_spring_prefix_for_unannotated_classes() {
    let source = "@RequestMapping(\"/api\")\npublic class ApiController {\n@GetMapping(\"/users\")\npublic String users() {\n}\npublic class HealthResource {\n@GetMapping(\"/health\")\npublic String health() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.url == "/api/users"));
    assert!(routes.iter().any(|route| route.url == "/health"));
    assert!(!routes.iter().any(|route| route.url == "/api/health"));
}

#[test]
fn detects_spring_request_mapping_method_arrays() {
    let source = "@RequestMapping(value = \"/submit\", method = {RequestMethod.GET, RequestMethod.POST})\npublic String submit() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.http_method == "get"));
    assert!(routes.iter().any(|route| route.http_method == "post"));
    assert!(routes.iter().all(|route| route.url == "/submit"));
}

#[test]
fn detects_spring_path_attribute_after_method_attribute() {
    let source = "@RequestMapping(method = RequestMethod.POST, value = \"/login\")\npublic String login() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/login");
    assert_eq!(routes[0].http_method, "post");
}

#[test]
fn detects_express_final_handler_after_middleware() {
    let source = "app.get('/users', requireAuth, auditRequest, listUsers);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn expands_express_path_arrays() {
    let source = "app.get(['/v1/users', '/v2/users'], listUsers);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.url == "/v1/users"));
    assert!(routes.iter().any(|route| route.url == "/v2/users"));
    assert!(routes.iter().all(|route| route.handler_name == "listUsers"));
}

#[test]
fn detects_express_final_handler_inside_callback_arrays() {
    let source = "app.get('/users', [requireAuth, listUsers]);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn detects_multiline_express_route_registration() {
    let source = "app.get(\n  '/users',\n  requireAuth,\n  listUsers\n);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "listUsers");
    assert_eq!(routes[0].line, 1);
}

#[test]
fn detects_express_router_mount_prefix() {
    let source = "app.use('/api', router);\nrouter.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn detects_express_member_expression_handler_leaf() {
    let source = "router.get('/users', userController.listUsers);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn detects_express_route_chains() {
    let source = "router.route('/users').get(listUsers).post(createUser);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/users" && route.http_method == "get" && route.handler_name == "listUsers"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/users" && route.http_method == "post" && route.handler_name == "createUser"
    }));
}

#[test]
fn detects_spring_methodless_request_mapping_as_any_method() {
    let source = "@RequestMapping(\"/status\")\npublic String status() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/status");
    assert_eq!(routes[0].http_method, "any");
}

#[test]
fn expands_spring_mapping_path_arrays() {
    let source = "@GetMapping({\"/v1/users\", \"/v2/users\"})\npublic List<User> users() {\n@RequestMapping(path = {\"/imports\", \"/exports\"}, method = RequestMethod.POST)\npublic String transfer() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 4);
    assert!(routes.iter().any(|route| {
        route.url == "/v1/users" && route.http_method == "get" && route.handler_name == "users"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/v2/users" && route.http_method == "get" && route.handler_name == "users"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/imports" && route.http_method == "post" && route.handler_name == "transfer"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/exports" && route.http_method == "post" && route.handler_name == "transfer"
    }));
}

#[test]
fn detects_multiline_spring_mapping_annotations() {
    let source = "@PostMapping(\n    path = \"/login\"\n)\npublic String login() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/login");
    assert_eq!(routes[0].http_method, "post");
    assert_eq!(routes[0].handler_name, "login");
}

#[test]
fn detects_flask_blueprint_url_prefix() {
    let source = "bp = Blueprint('api', __name__, url_prefix='/api')\n@bp.route('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].handler_name, "users");
}

#[test]
fn detects_multiline_flask_route_decorators() {
    let source = "@app.route(\n    '/login',\n    methods=['POST'],\n)\ndef login():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/login");
    assert_eq!(routes[0].http_method, "post");
    assert_eq!(routes[0].handler_name, "login");
}

#[test]
fn detects_multiline_express_route_chains() {
    let source = "router.route('/users')\n  .get(listUsers)\n  .post(createUser);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .any(|route| route.http_method == "get" && route.handler_name == "listUsers")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.http_method == "post" && route.handler_name == "createUser")
    );
}

#[test]
fn bounds_semicolon_free_express_route_chains() {
    let source =
        "router.route('/users')\n  .get(listUsers)\nrouter.route('/items')\n  .post(createItem)\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/users" && route.http_method == "get" && route.handler_name == "listUsers"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/items" && route.http_method == "post" && route.handler_name == "createItem"
    }));
    assert!(
        !routes
            .iter()
            .any(|route| route.url == "/users" && route.http_method == "post")
    );
}

#[test]
fn detects_express_router_alias_assignments() {
    let source = "const users = express.Router();\napp.use('/api', users);\nusers.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn detects_express_application_aliases() {
    let source = "const server = express();\nserver.get('/health', health);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].handler_name, "health");
}

#[test]
fn detects_multiline_express_router_mount_prefix() {
    let source = "app.use(\n  '/api',\n  router\n);\nrouter.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
}

#[test]
fn skips_commented_express_registrations() {
    let source = "// app.get('/users', listUsers);\n/* router.post('/admin', admin); */\napp.get('/live', live);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
}

#[test]
fn detects_express_all_head_and_options_methods() {
    let source = "app.all('/any', anyHandler);\napp.head('/head', headHandler);\napp.options('/cors', corsHandler);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 3);
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/any" && route.http_method == "any")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/head" && route.http_method == "head")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/cors" && route.http_method == "options")
    );
}

#[test]
fn detects_jsx_express_routes() {
    let routes = detect_routes("jsx", "app.get('/status', status);\n");
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/status");
}

#[test]
fn detects_multiline_fastapi_router_prefix() {
    let source = "router = APIRouter(\n    prefix='/api',\n)\n@router.get('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
}

#[test]
fn detects_multiline_flask_blueprint_prefix() {
    let source = "bp = Blueprint(\n    'api',\n    __name__,\n    url_prefix='/api',\n)\n@bp.route('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
}

#[test]
fn detects_fastapi_head_and_options_decorators() {
    let source = "@router.head('/ready')\ndef ready():\n    pass\n@router.options('/ready')\ndef options():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/ready" && route.http_method == "head")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/ready" && route.http_method == "options")
    );
}

#[test]
fn applies_late_express_router_mount_prefix() {
    let source = "const router = express.Router();\nrouter.get('/users', listUsers);\napp.use('/api', router);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn labels_fastapi_application_decorators() {
    let source = "app = FastAPI()\n@app.get('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].framework, "fastapi");
}

#[test]
fn merges_flask_register_blueprint_prefix() {
    let source = "bp = Blueprint('api', __name__)\napp.register_blueprint(bp, url_prefix='/api')\n@bp.route('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].framework, "flask");
}

#[test]
fn applies_late_fastapi_include_router_prefix() {
    let source = "router = APIRouter()\n@router.get('/users')\ndef users():\n    pass\napp.include_router(router, prefix='/api')\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].framework, "fastapi");
}

#[test]
fn skips_python_routes_inside_triple_quoted_strings() {
    let source = "\"\"\"\n@app.get('/demo')\ndef demo():\n    pass\n\"\"\"\n@app.get('/live')\ndef live():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
    assert_eq!(routes[0].handler_name, "live");
}

#[test]
fn detects_fully_qualified_spring_mapping_annotations() {
    let source = "@org.springframework.web.bind.annotation.GetMapping(\"/health\")\npublic String health() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].http_method, "get");
}

#[test]
fn preserves_spring_prefix_after_nested_static_type() {
    let source = "@RequestMapping(\"/api\")\npublic class ApiController {\nstatic class Helper {\n}\n@GetMapping(\"/users\")\npublic String users() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
}

#[test]
fn preserves_all_fastapi_router_mount_prefixes() {
    let source = "router = APIRouter()\n@router.get('/users')\ndef users():\n    pass\napp.include_router(router, prefix='/v1')\napp.include_router(router, prefix='/v2')\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.url == "/v1/users"));
    assert!(routes.iter().any(|route| route.url == "/v2/users"));
}

#[test]
fn skips_express_routes_inside_strings() {
    let source = "const doc = \"app.get('/demo', demo);\";\napp.get('/live', live);\n";
    let routes = detect_routes("javascript", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
    assert_eq!(routes[0].handler_name, "live");
}

#[test]
fn mounts_every_router_passed_to_express_use() {
    let source = "const authRouter = express.Router();\nconst usersRouter = express.Router();\nauthRouter.get('/login', login);\nusersRouter.get('/users', listUsers);\napp.use('/api', authRouter, usersRouter);\n";
    let routes = detect_routes("typescript", source);
    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/api/login" && route.handler_name == "login")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/api/users" && route.handler_name == "listUsers")
    );
}

#[test]
fn skips_spring_mappings_inside_block_comments() {
    let source = "/*\n@GetMapping(\"/old\")\npublic String old() {\n}\n*/\n@GetMapping(\"/live\")\npublic String live() {\n";
    let routes = detect_routes("java", source);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
    assert_eq!(routes[0].handler_name, "live");
}

#[test]
fn accepts_python_keyword_route_paths() {
    let source = "router = APIRouter(prefix='/api')\n@app.route(rule='/users', methods=['POST'])\ndef users():\n    pass\n@router.get(path='/items')\ndef items():\n    pass\n";
    let routes = detect_routes("python", source);
    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/users" && route.http_method == "post")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/api/items" && route.http_method == "get")
    );
}
