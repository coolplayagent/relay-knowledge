use super::{JavaImportRequest, resolve};
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
