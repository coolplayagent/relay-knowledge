use super::*;

#[test]
fn cgo_lossy_display_signatures_cannot_prove_external_linkage() {
    let long_attribute = format!(
        "__attribute__((annotate(\"{}\"))) static int decode(void) {{ return 0; }}",
        "long annotation ".repeat(50)
    );
    for source in [
        "int // note\n static decode(void) { return 0; }",
        long_attribute.as_str(),
    ] {
        let snapshot = parse_sources(&[
            (
                "bridge.GO",
                "package bridge\nimport \"C\"\nfunc run() { C.decode() }",
            ),
            ("private.c", source),
        ]);
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.name == "decode"),
            "{source}"
        );
        assert!(
            snapshot.calls.iter().any(|call| call.path == "bridge.GO"
                && call.target_hint.as_deref() == Some("C.decode")
                && call.resolution_state == "unresolved"),
            "{source}: {:?}",
            snapshot.calls
        );
    }
}

#[test]
fn cgo_does_not_resolve_an_external_symbol_to_an_unrelated_static_definition() {
    let snapshot = parse_sources(&[
        (
            "bridge.go",
            "package bridge\n/* extern int decode(void); */\nimport \"C\"\nfunc run() { C.decode() }",
        ),
        ("private.c", "static int decode(void) { return 0; }"),
    ]);
    assert!(snapshot.calls.iter().any(|call| call.path == "bridge.go"
        && call.target_hint.as_deref() == Some("C.decode")
        && call.resolution_state == "unresolved"));
}

#[test]
fn excluded_shebang_files_cannot_resolve_import_targets() {
    let registration =
        CodeRepositoryRegistration::new("repo", "repo", "/tmp/repo", vec![], vec!["bash".into()])
            .unwrap();
    let mut build = SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 2, 0);
    parse_indexed_file(&mut build, "main.sh", b"source ./foreign\n").unwrap();
    parse_indexed_file(&mut build, "foreign", b"#!/usr/bin/env python3\nprint(1)\n").unwrap();
    let snapshot = build.finish();
    assert_eq!(
        snapshot
            .files
            .iter()
            .find(|file| file.path == "foreign")
            .unwrap()
            .parse_status,
        CodeParseStatus::Excluded
    );
    assert!(!snapshot.imports.is_empty());
    assert!(
        snapshot
            .imports
            .iter()
            .all(|import| import.resolution_state != "resolved")
    );
}

#[test]
fn cgo_receiver_requires_import_and_unshadowed_binding_and_c_target() {
    for source in [
        "package bridge\nimport \"C\"\nfunc run(x, C Client) { C.decode() }",
        "package bridge\nimport \"C\"\nfunc run() { var x, C Client; C.decode() }",
        "package bridge\nimport \"C\"\nimport C \"other\"\nfunc run() { C.decode() }",
        "package bridge\nimport \"C\"\nfunc run() { type C = Client; C.decode() }",
        "package bridge\nimport \"C\"\nfunc run() { select { case C := <-ch: C.decode() } }",
        "package bridge\nimport \"C\"\nfunc run() { switch C := x.(type) { case Client: C.decode() } }",
        "package bridge\nfunc run(C Client) { C.decode() }",
        "package bridge\nimport \"C\"\nfunc run(C Client) { C.decode() }",
        "package bridge\nimport \"C\"\nfunc run() { C := factory(); C.decode() }",
        "package bridge\nimport \"C\"\nfunc run() { var C Client; C.decode() }",
    ] {
        let snapshot = parse_sources(&[("bridge.go", source), ("decode.c", "void decode() {}")]);
        assert!(
            snapshot.calls.iter().any(|call| call.path == "bridge.go"
                && call.target_hint.as_deref() == Some("C.decode")
                && call.resolution_state == "unresolved"),
            "{source}: {:?}",
            snapshot.calls
        );
    }
    let snapshot = parse_sources(&[
        (
            "bridge.go",
            "package bridge\nimport \"C\"\nfunc run() { C.decode() }",
        ),
        ("other.go", "package other\nfunc decode() {}"),
        ("other.js", "function decode() {}"),
    ]);
    assert!(snapshot.calls.iter().any(|call| call.path == "bridge.go"
        && call.target_hint.as_deref() == Some("C.decode")
        && call.resolution_state == "unresolved"));
}
