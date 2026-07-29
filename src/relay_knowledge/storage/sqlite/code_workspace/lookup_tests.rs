use super::code_workspace::workspace_lookup_module;

#[test]
fn workspace_lookup_module_normalizes_language_import_statements() {
    assert_eq!(
        [
            workspace_lookup_module("api \"example.com/svc/api\"", "go"),
            workspace_lookup_module("_ `example.com/svc/api`;", "go"),
            workspace_lookup_module("import { x } from \"@scope/core\";", "npm"),
            workspace_lookup_module("await import('@scope/core/client')", "npm"),
            workspace_lookup_module("pub use core::client::Client;", "rust"),
            workspace_lookup_module("extern crate core as core_alias;", "rust"),
            workspace_lookup_module("use crate::{local};", "rust"),
        ],
        [
            "example.com/svc/api",
            "example.com/svc/api",
            "@scope/core",
            "@scope/core/client",
            "core::client::Client",
            "core",
            "crate",
        ]
    );
}
