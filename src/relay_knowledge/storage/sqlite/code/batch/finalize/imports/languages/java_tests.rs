use super::{JavaImportRequest, imported_symbol_names, resolve};
use crate::storage::sqlite::code::batch::finalize::imports::ImportResolution;

#[test]
fn java_import_parser_separates_class_and_static_member_requests() {
    assert_eq!(
        JavaImportRequest::parse("import com.example.Widget;"),
        Some(JavaImportRequest::Class {
            class_path: "com/example/Widget".to_owned(),
        })
    );
    assert_eq!(
        JavaImportRequest::parse("import static com.example.Widget.create;"),
        Some(JavaImportRequest::StaticMember {
            class_path: "com/example/Widget".to_owned(),
            member: "create".to_owned(),
        })
    );
}

#[test]
fn java_symbol_dependencies_only_include_static_members() {
    assert_eq!(
        imported_symbol_names("import static com.example.Widget.create;"),
        vec!["create"]
    );
    assert!(imported_symbol_names("import com.example.Widget;").is_empty());
}

#[test]
fn malformed_java_imports_remain_unresolved() {
    assert_eq!(
        resolve(
            "package com.example;",
            &Default::default(),
            &Default::default(),
        ),
        ImportResolution::Unresolved
    );
}
