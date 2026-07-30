use super::detect_express_routes;

#[test]
fn detector_composes_alias_mount_and_registration_owners() {
    let source = "
        import express, { Router as ExpressRouter } from 'express';
        const app = express();
        const users = ExpressRouter();
        app.use('/api', users);
        users.get('/users', requireAuth, userController.listUsers);
    ";

    let routes = detect_express_routes(source);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].http_method, "get");
    assert_eq!(routes[0].handler_name, "userController.listUsers");
    assert_eq!(routes[0].framework, "express");
}
