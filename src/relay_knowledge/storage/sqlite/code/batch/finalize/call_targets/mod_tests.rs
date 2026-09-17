//! Direct tests for bounded call-target disambiguation.

use super::{CallTargetSymbol, unique_preferred_callable};

#[test]
fn java_static_targets_use_exact_ownership_without_same_path_or_leaf_fallback() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        (
            "A.java",
            "package demo; class A {void run(){B.process();}} class B {static void process(){}}",
        ),
        ("B.java", "package demo; class B {static void process(){}}"),
        (
            "Single.java",
            "package demo; class Single {static void process(){}}",
        ),
        (
            "Instance.java",
            "package demo; class Instance {void process(){}}",
        ),
        ("foreign.js", "class Single {static process(){}}"),
    ]);
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE code_repository_symbols(source_scope TEXT,symbol_snapshot_id TEXT,path TEXT,name TEXT,kind TEXT,signature TEXT,language_id TEXT,type_owner_json TEXT)").unwrap();
    for symbol in snapshot.symbols {
        db.execute(
            "INSERT INTO code_repository_symbols VALUES('scope',?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                symbol.symbol_snapshot_id,
                symbol.path,
                symbol.name,
                symbol.kind,
                symbol.signature,
                symbol.language_id,
                symbol
                    .type_owner
                    .as_ref()
                    .map(|owner| serde_json::to_string(owner).unwrap())
            ],
        )
        .unwrap();
    }
    let tx = db.transaction().unwrap();
    let index = super::CallTargetIndex::load(&tx, "scope").unwrap();
    assert!(matches!(
        index.resolve("demo.B.process", "A.java"),
        super::TargetResolution::Ambiguous(_)
    ));
    assert!(
        matches!(index.resolve("demo.Single.process","A.java"),super::TargetResolution::Resolved(symbol,_) if symbol.path=="Single.java")
    );
    for name in [
        "demo.Instance.process",
        "other.Single.process",
        "Unknown.process",
    ] {
        assert!(
            matches!(
                index.resolve(name, "A.java"),
                super::TargetResolution::Unresolved
            ),
            "{name}"
        );
    }
}

#[test]
fn code_index_persistence_performance_suite_cgo_uses_preselected_visible_targets() {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE code_repository_symbols (
        source_scope TEXT, symbol_snapshot_id TEXT, path TEXT, name TEXT,
        kind TEXT, signature TEXT, language_id TEXT);
        INSERT INTO code_repository_symbols VALUES
        ('scope','private','private.c','private_fn','function','static int private_fn(void) {','c'),
        ('scope','inherited-decl','private.c','inherited_fn','function_declaration','static int inherited_fn(void);','c'),
        ('scope','inherited-def','private.c','inherited_fn','function','int inherited_fn(void) {','c'),
        ('scope','wrong-language','other.go','other_fn','function','func other_fn() {','go'),
        ('scope','public','api.c','public_fn','function','int public_fn(int a[static 1]) { static int cache; }','c'),
        ('scope','decl','api.h','public_fn','function_declaration','int public_fn(void);','c');",
    )
    .unwrap();
    for i in 0..2_000 {
        db.execute("INSERT INTO code_repository_symbols VALUES ('scope',?1,'duplicate.c','duplicate_fn','function','int duplicate_fn(void) {','c')", [i.to_string()]).unwrap();
    }
    db.execute_batch("ALTER TABLE code_repository_symbols ADD COLUMN type_owner_json TEXT")
        .unwrap();
    let tx = db.transaction().unwrap();
    let index = super::CallTargetIndex::load(&tx, "scope").unwrap();
    for _ in 0..10_000 {
        assert!(matches!(
            index.resolve("C.inherited_fn", "bridge.GO"),
            super::TargetResolution::Unresolved
        ));
        assert!(matches!(
            index.resolve("C.private_fn", "bridge.go"),
            super::TargetResolution::Unresolved
        ));
        assert!(matches!(
            index.resolve("C.other_fn", "bridge.GO"),
            super::TargetResolution::Unresolved
        ));
        assert!(matches!(
            index.resolve("C.duplicate_fn", "bridge.go"),
            super::TargetResolution::Ambiguous(_)
        ));
        assert!(
            matches!(index.resolve("C.public_fn", "bridge.go"), super::TargetResolution::Resolved(symbol, _) if symbol.symbol_snapshot_id == "public")
        );
    }
}

#[test]
fn preferred_callable_selects_the_only_definition_over_declarations() {
    let symbols = [
        symbol("declaration", "function", "int connect();"),
        symbol("definition", "function", "int connect() {"),
    ];

    let preferred = unique_preferred_callable(&symbols).expect("definition should be unique");

    assert_eq!(preferred.symbol_snapshot_id, "definition");
}

fn symbol(symbol_snapshot_id: &str, kind: &str, signature: &str) -> CallTargetSymbol {
    CallTargetSymbol {
        language_id: "c".into(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        path: "src/client.c".to_owned(),
        kind: kind.to_owned(),
        signature: signature.to_owned(),
    }
}
