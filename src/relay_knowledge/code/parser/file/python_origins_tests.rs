use crate::code::{SnapshotBuild, parser::parse_indexed_file, python_imports::PythonModuleOrigins};
use crate::domain::CodeRepositoryRegistration;
#[test]
fn parser_uses_scope_origin_without_changing_symbol_ranges_or_other_provider() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", vec![], vec![]).unwrap();
    // Re-establish the other provider after the unproven local decorator call.
    // That call could mutate a previously imported module binding.
    let source = "import typing\n@typing.overload\ndef local(x:int): ...\nimport typing_extensions as ext\n@ext.overload\ndef external(x:int): ...\n";
    let mut build = SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
    build.python_module_origins =
        PythonModuleOrigins::from_authorized_paths(["typing.py", "app.py"], &[], &[]);
    parse_indexed_file(&mut build, "app.py", source.as_bytes()).unwrap();
    let snapshot = build.finish();
    let local = snapshot.symbols.iter().find(|s| s.name == "local").unwrap();
    let external = snapshot
        .symbols
        .iter()
        .find(|s| s.name == "external")
        .unwrap();
    assert_eq!(local.kind, "function");
    assert_eq!(external.kind, "function_declaration");
    assert_eq!(
        local.byte_range.start as usize,
        source.find("def local").unwrap()
    );
    assert_eq!(
        external.byte_range.start as usize,
        source.find("def external").unwrap()
    );
}
