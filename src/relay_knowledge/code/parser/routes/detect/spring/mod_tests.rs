use super::detect_spring_routes;

#[test]
fn detector_composes_class_prefixes_and_method_mappings() {
    let source = r#"
        @RequestMapping("/api")
        class UserController {
            @GetMapping(path = {"/users", "/members"})
            public String listUsers() {
                return "ok";
            }
        }
    "#;

    let routes = detect_spring_routes(source);

    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "listUsers");
    assert_eq!(routes[0].framework, "spring");
    assert_eq!(routes[1].url, "/api/members");
}
