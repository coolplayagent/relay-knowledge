use super::manual_file_definitions;

#[test]
fn manual_file_definitions_recover_same_line_and_exported_class_members() {
    let definitions = manual_file_definitions(
        r#"
class Compact { public: void Bar(); };
LEVELDB_EXPORT class ExportedDB {
 public:
  __attribute__((warn_unused_result)) Status Open();
};
"#,
    );

    assert!(definitions.iter().any(|(name, qualified, kind, _)| {
        name == "Bar"
            && qualified.as_deref() == Some("Compact.Bar")
            && *kind == "function_declaration"
    }));
    assert!(definitions.iter().any(|(name, qualified, kind, _)| {
        name == "Open"
            && qualified.as_deref() == Some("ExportedDB.Open")
            && *kind == "function_declaration"
    }));
}
