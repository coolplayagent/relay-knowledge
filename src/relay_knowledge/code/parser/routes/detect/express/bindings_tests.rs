use std::collections::BTreeSet;

use super::{
    express_namespace_names, express_router_factory_names, js_assignment_variable_name,
    parse_express_application_alias, parse_express_router_alias,
};

#[test]
fn discovers_esm_and_commonjs_router_factory_bindings() {
    let source = "
        import { Router, Router as ExpressRouter } from 'express';
        const { Router: CommonRouter } = require('express');
        import { Router as OtherRouter } from 'other';
    ";

    assert_eq!(
        express_router_factory_names(source),
        BTreeSet::from([
            "CommonRouter".to_owned(),
            "ExpressRouter".to_owned(),
            "Router".to_owned(),
        ])
    );
}

#[test]
fn discovers_default_namespace_and_required_express_names() {
    let source = "
        import web from 'express';
        import * as expressNamespace from \"express\";
        const common$ = require(`express`);
        const fake = \"import ignored from 'express'\";
    ";

    assert_eq!(
        express_namespace_names(source),
        BTreeSet::from([
            "common$".to_owned(),
            "express".to_owned(),
            "expressNamespace".to_owned(),
            "web".to_owned(),
        ])
    );
}

#[test]
fn assignment_binding_accepts_exported_and_typed_identifiers() {
    assert_eq!(
        js_assignment_variable_name("export const users: Router"),
        Some("users".to_owned())
    );
    assert_eq!(
        js_assignment_variable_name("let api$"),
        Some("api$".to_owned())
    );
    assert_eq!(js_assignment_variable_name("const users-router"), None);
}

#[test]
fn alias_parsing_requires_known_express_factories() {
    let express_names = BTreeSet::from(["express".to_owned(), "web".to_owned()]);
    let router_factories = BTreeSet::from(["ExpressRouter".to_owned()]);

    assert_eq!(
        parse_express_application_alias("const app = web()", &express_names),
        Some("app".to_owned())
    );
    assert_eq!(
        parse_express_router_alias(
            "export const users = ExpressRouter()",
            &router_factories,
            &express_names,
        ),
        Some("users".to_owned())
    );
    assert_eq!(
        parse_express_router_alias(
            "const fake = OtherRouter()",
            &router_factories,
            &express_names,
        ),
        None
    );
}
