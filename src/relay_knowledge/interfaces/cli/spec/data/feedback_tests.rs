#[test]
fn publication_commands_declare_operational_writes() {
    let specs = super::command_specs();
    assert_eq!(specs.len(), 9);
    for spec in specs {
        let json = serde_json::to_value(spec).unwrap();
        if matches!(
            json["path"][1].as_str(),
            Some("submit" | "retry" | "report")
        ) {
            assert_eq!(json["effect"], "writes-operational-state");
        }
    }
}
