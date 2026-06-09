use super::detect::detect_routes;

#[test]
fn preserves_distinct_spring_handlers_for_same_route() {
    let source = "@GetMapping(value = \"/search\", params = \"q\")\npublic String searchByQuery() {\n}\n@GetMapping(value = \"/search\", params = \"id\")\npublic String searchById() {\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/search"
            && route.http_method == "get"
            && route.handler_name == "searchByQuery"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/search" && route.http_method == "get" && route.handler_name == "searchById"
    }));
}

#[test]
fn preserves_distinct_python_handlers_for_same_route() {
    let source = "@app.get('/search')\ndef search_by_query():\n    pass\n@app.get('/search')\ndef search_by_id():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/search"
            && route.http_method == "get"
            && route.handler_name == "search_by_query"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/search" && route.http_method == "get" && route.handler_name == "search_by_id"
    }));
}

#[test]
fn keeps_python_route_decorators_across_blank_and_comment_lines() {
    let source =
        "@app.get('/health')\n\n# operational health endpoint\ndef health():\n    return 'ok'\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].handler_name, "health");
}

#[test]
fn keeps_spring_mapping_across_line_comments() {
    let source =
        "@GetMapping(\"/health\")\n// operational health endpoint\npublic String health() {\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].handler_name, "health");
}

#[test]
fn skips_unproven_express_router_suffix_receivers() {
    let source = "settingsRouter.get('/theme', loadTheme);\n";
    let routes = detect_routes("typescript", source);

    assert!(routes.is_empty());
}

#[test]
fn accepts_mounted_express_router_suffix_receivers() {
    let source = "app.use('/api', usersRouter);\nusersRouter.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn detects_flask_add_url_rule_registrations() {
    let source = "app.add_url_rule('/health', view_func=health, methods=['GET', 'POST'])\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "health"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "post" && route.handler_name == "health"
    }));
}

#[test]
fn detects_flask_add_url_rule_positional_handlers() {
    let source = "app.add_url_rule('/health', 'health', health)\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "health");
}

#[test]
fn detects_prefixed_python_route_strings() {
    let source = "@app.get(r'/health')\ndef health():\n    pass\n@router.get(u\"/users\")\ndef users():\n    pass\napp.add_url_rule(r'/submit', view_func=submit)\n@app.get(f'/dynamic/{tenant}')\ndef dynamic():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 3);
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "health"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/users" && route.http_method == "get" && route.handler_name == "users"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/submit" && route.http_method == "get" && route.handler_name == "submit"
    }));
    assert!(!routes.iter().any(|route| route.url.contains("dynamic")));
}

#[test]
fn keeps_express_inline_final_handlers_anonymous() {
    let source = "app.get('/admin', requireAuth, (req, res) => res.send());\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/admin");
    assert_eq!(routes[0].handler_name, "anonymous");
}

#[test]
fn keeps_express_identifier_arrow_handlers_anonymous() {
    let source =
        "app.get('/health', req => send(req));\napp.post('/health', async req => send(req));\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "anonymous"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "post" && route.handler_name == "anonymous"
    }));
}

#[test]
fn skips_express_routes_inside_multiline_template_literals() {
    let source = "const doc = `\napp.get('/documented', documentedHandler);\n`;\napp.get('/live', liveHandler);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
    assert_eq!(routes[0].handler_name, "liveHandler");
}

#[test]
fn skips_dynamic_express_template_route_paths() {
    let source = "app.get(`${base}/users`, listUsers);\napp.post(`/api/${tenant}/users`, createUser);\napp.get('/live', liveHandler);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
    assert_eq!(routes[0].handler_name, "liveHandler");
}

#[test]
fn preserves_express_member_expression_handler_targets() {
    let source = "router.get('/users', usersController.list);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].handler_name, "usersController.list");
}

#[test]
fn detects_multiple_express_registrations_on_one_line() {
    let source = "app.get('/alpha', alpha); app.post('/beta', beta);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/alpha" && route.http_method == "get" && route.handler_name == "alpha"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/beta" && route.http_method == "post" && route.handler_name == "beta"
    }));
}

#[test]
fn keeps_same_line_express_route_chain_separate_from_following_registration() {
    let source =
        "router.route('/health').get(requireAuth).get(health); app.post('/login', login);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 3);
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "requireAuth"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "health"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/login" && route.http_method == "post" && route.handler_name == "login"
    }));
    assert!(!routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "post" && route.handler_name == "login"
    }));
}

#[test]
fn detects_multiple_same_line_express_route_chains() {
    let source = "router.route('/alpha').get(alpha); router.route('/beta').post(beta);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/alpha" && route.http_method == "get" && route.handler_name == "alpha"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/beta" && route.http_method == "post" && route.handler_name == "beta"
    }));
}

#[test]
fn detects_express_namespace_import_router_factories() {
    let source = "import * as web from 'express';\nconst api = web.Router();\napi.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn detects_aliased_express_router_import_factories() {
    let source = "import { Router as ExpressRouter } from 'express';\nconst api = ExpressRouter();\napi.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn continues_after_same_line_express_mounts() {
    let source = "const router = express.Router();\nrouter.get('/users', listUsers);\napp.use('/api', router); app.get('/health', health);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/api/users" && route.http_method == "get" && route.handler_name == "listUsers"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "health"
    }));
}

#[test]
fn detects_multiple_same_line_express_mounts() {
    let source = "const aRouter = express.Router();\nconst bRouter = express.Router();\naRouter.get('/alpha', alpha);\nbRouter.get('/beta', beta);\napp.use('/a', aRouter); app.use('/b', bRouter);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/a/alpha" && route.http_method == "get" && route.handler_name == "alpha"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/b/beta" && route.http_method == "get" && route.handler_name == "beta"
    }));
}

#[test]
fn detects_multiple_same_line_spring_mapping_annotations() {
    let source = "@GetMapping(\"/alpha\") @PostMapping(\"/beta\") public String handle() {\n}\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/alpha" && route.http_method == "get" && route.handler_name == "handle"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/beta" && route.http_method == "post" && route.handler_name == "handle"
    }));
}

#[test]
fn detects_python_set_method_lists() {
    let source = "@app.route('/submit', methods={'POST'})\ndef submit():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/submit");
    assert_eq!(routes[0].http_method, "post");
}

#[test]
fn dynamic_flask_methods_are_recorded_as_any() {
    let source = "ALLOWED_METHODS = ['POST']\n@app.route('/submit', methods=ALLOWED_METHODS)\ndef submit():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/submit");
    assert_eq!(routes[0].http_method, "any");
    assert_eq!(routes[0].handler_name, "submit");
}

#[test]
fn expands_express_array_mount_paths() {
    let source = "app.use(['/api', '/v1'], router);\nrouter.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| route.url == "/api/users"));
    assert!(routes.iter().any(|route| route.url == "/v1/users"));
    assert!(routes.iter().all(|route| route.handler_name == "listUsers"));
}

#[test]
fn detects_express_routes_after_same_line_mounts() {
    let source =
        "app.use('/api', router); app.get('/health', health);\nrouter.get('/users', listUsers);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "health"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/api/users" && route.http_method == "get" && route.handler_name == "listUsers"
    }));
}

#[test]
fn detects_spring_mapping_after_same_line_annotation() {
    let source =
        "@PreAuthorize(\"hasRole('ADMIN')\") @GetMapping(\"/admin\") public String admin() {\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/admin");
    assert_eq!(routes[0].handler_name, "admin");
}

#[test]
fn detects_spring_mapping_before_same_line_non_route_annotation() {
    let source =
        "@GetMapping(\"/admin\") @PreAuthorize(\"hasRole('ADMIN')\") public String admin() {\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/admin");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "admin");
}

#[test]
fn keeps_python_route_decorators_across_multiline_non_route_decorators() {
    let source =
        "@app.get('/admin')\n@requires_permission(\n    'admin',\n)\ndef admin():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/admin");
    assert_eq!(routes[0].handler_name, "admin");
}

#[test]
fn keeps_spring_mapping_across_multiline_non_route_annotations() {
    let source = "@GetMapping(\"/admin\")\n@PreAuthorize(\n    \"hasRole('ADMIN')\"\n)\npublic String admin() {\n}\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/admin");
    assert_eq!(routes[0].handler_name, "admin");
}

#[test]
fn skips_express_routes_inside_regex_literals() {
    let source = "const re = /app.get('\\/demo', demo)/;\napp.get('/live', live);\n";
    let routes = detect_routes("typescript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
    assert_eq!(routes[0].handler_name, "live");
}

#[test]
fn ignores_express_router_import_evidence_inside_comments_and_strings() {
    let source = "// import { Router } from 'express'\nconst doc = \"const Router = require('express')\";\nconst api = Router();\napi.get('/fake', fake);\n";
    let routes = detect_routes("typescript", source);

    assert!(routes.is_empty());
}

#[test]
fn uses_python_route_keyword_boundaries_for_include_prefix() {
    let source = "router = APIRouter()\napp = FastAPI()\napp.include_router(router, tags=[\"prefix='/debug'\"], prefix=\"/api\")\n@router.get('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert!(!routes.iter().any(|route| route.url == "/debug/users"));
}

#[test]
fn skips_unmounted_declared_python_routers() {
    let source = "router = APIRouter(prefix='/internal')\n@router.get('/health')\ndef health():\n    pass\nbp = Blueprint('admin', __name__, url_prefix='/admin')\n@bp.route('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);

    assert!(routes.is_empty());
}

#[test]
fn skips_dynamic_python_router_mount_prefixes() {
    let source = "router = APIRouter()\napp.include_router(router, prefix=api_prefix)\n@router.get('/users')\ndef users():\n    pass\nlocal_router = APIRouter(prefix=local_prefix)\napp.include_router(local_router)\n@local_router.get('/items')\ndef items():\n    pass\nbp = Blueprint('admin', __name__)\napp.register_blueprint(bp, url_prefix=admin_prefix)\n@bp.route('/users')\ndef admin_users():\n    pass\ndynamic_bp = Blueprint('dynamic', __name__, url_prefix=dynamic_prefix)\napp.register_blueprint(dynamic_bp)\n@dynamic_bp.route('/settings')\ndef settings():\n    pass\n";
    let routes = detect_routes("python", source);

    assert!(routes.is_empty());
}

#[test]
fn detects_keyword_fastapi_router_mounts() {
    let source = "users_router = APIRouter()\napp.include_router(router=users_router, prefix='/api')\n@users_router.get('/users')\ndef users():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
}

#[test]
fn prefers_python_route_path_keywords_over_other_strings() {
    let source = "@app.get(name='users', path='/api/users')\ndef users():\n    pass\napp.add_url_rule(endpoint='status', rule='/api/status', view_func=status)\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/api/users" && route.handler_name == "users")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/api/status" && route.handler_name == "status")
    );
    assert!(!routes.iter().any(|route| route.url == "users"));
    assert!(!routes.iter().any(|route| route.url == "status"));
}

#[test]
fn keeps_python_routes_before_multiline_non_route_decorators() {
    let source =
        "@app.get('/admin')\n@requires_permission(\n    'admin',\n)\ndef admin():\n    pass\n";
    let routes = detect_routes("python", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/admin");
    assert_eq!(routes[0].handler_name, "admin");
}

#[test]
fn keeps_spring_routes_before_multiline_non_route_annotations() {
    let source = "@GetMapping(\"/admin\")\n@PreAuthorize(\n    \"hasRole('ADMIN')\"\n)\npublic String admin() {\n}\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/admin");
    assert_eq!(routes[0].handler_name, "admin");
}

#[test]
fn preserves_class_level_spring_request_methods() {
    let source = "@RequestMapping(value = \"/api\", method = RequestMethod.POST)\npublic class UsersController {\n@GetMapping(\"/users\")\npublic String users() {\n}\n}\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|route| {
        route.url == "/api/users" && route.http_method == "get" && route.handler_name == "users"
    }));
    assert!(routes.iter().any(|route| {
        route.url == "/api/users" && route.http_method == "post" && route.handler_name == "users"
    }));
}

#[test]
fn ignores_spring_routes_inside_java_text_blocks() {
    let source = "String docs = \"\"\"\n@GetMapping(\"/documented\")\npublic String documented() {\n}\n\"\"\";\n@GetMapping(\"/live\")\npublic String live() {\n}\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/live");
    assert_eq!(routes[0].handler_name, "live");
}

#[test]
fn preserves_spring_prefixes_across_public_nested_types() {
    let source = "@RequestMapping(\"/api\")\npublic class UsersController {\npublic static class Helper {\n}\n@GetMapping(\"/users\")\npublic String users() {\n}\n}\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].handler_name, "users");
}

#[test]
fn ignores_spring_attribute_names_inside_string_values() {
    let source =
        "@RequestMapping(value = \"/health\", params = \"method=POST\") public String health() {\n";
    let routes = detect_routes("java", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/health");
    assert_eq!(routes[0].http_method, "any");
    assert_eq!(routes[0].handler_name, "health");
}

#[test]
fn detects_inline_commonjs_express_router_factories() {
    let source = "const api = require('express').Router();\napi.get('/users', listUsers);\n";
    let routes = detect_routes("javascript", source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/users");
    assert_eq!(routes[0].handler_name, "listUsers");
}

#[test]
fn skips_dynamic_express_router_mount_prefixes() {
    let source = "const usersRouter = express.Router();\napp.use(apiPrefix, usersRouter);\nusersRouter.get('/users', listUsers);\n";
    let routes = detect_routes("javascript", source);

    assert!(routes.is_empty());
}
