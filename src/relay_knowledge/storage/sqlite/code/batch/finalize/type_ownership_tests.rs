use super::*;
use crate::domain::{CodeIndexResourceBudget, CodeIndexSnapshot};

#[test]
fn type_ownership_empty_fact_set_preserves_small_function_index_budgets() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("plain.c", "void run() {}")]);
    assert!(!snapshot.symbols.is_empty());
    assert!(
        snapshot
            .symbols
            .iter()
            .all(|symbol| symbol.type_owner.is_none())
    );
    let (mut db, mut session) = database(&snapshot);
    session.resource_budget = CodeIndexResourceBudget::new(1, 1024, 4).unwrap();
    let before = db.total_changes();
    let tx = db.transaction().unwrap();
    assert!(advance(&tx, &session).unwrap());
    tx.commit().unwrap();
    assert_eq!(db.total_changes(), before);
}

#[test]
fn type_ownership_resume_rejects_checkpoint_cursor_disagreement() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("owner.rs", "struct Owner;")]);
    let (mut db, session) = database(&snapshot);
    let state = crate::domain::CodeQueryIndexRepairResumePhase::ownership_checkpoint_state("wrong")
        .unwrap();
    db.execute(
        "UPDATE code_repository_index_checkpoints SET type_owner_cursor='different',state=?1",
        [state],
    )
    .unwrap();
    let tx = db.transaction().unwrap();
    assert!(matches!(
        advance(&tx, &session),
        Err(StorageError::Invariant(_))
    ));
    tx.rollback().unwrap();
}

#[test]
fn type_ownership_template_arguments_require_binding_evidence() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "owner.cpp",
        "namespace ns { struct V {}; template<class T> struct Owner { void run(); }; template<> struct Owner<V> { void run(); }; } struct V {}; template<> void ns::Owner<V>::run() {}",
    )]);
    let member = snapshot
        .symbols
        .iter()
        .find(|s| s.name == "run" && s.signature.contains('{'))
        .unwrap();
    let parsed = member.type_owner.as_ref().unwrap();
    assert_eq!(parsed.resolution_state.as_deref(), Some("unresolved"));
    assert_eq!(parsed.target_hint, "ns.Owner<V>");
    let id = member.symbol_snapshot_id.clone();
    let (mut db, session) = database(&snapshot);
    finish(&mut db, &session);
    let metadata: String = db
        .query_row(
            "SELECT type_owner_json FROM code_repository_symbols WHERE symbol_snapshot_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let persisted: CodeTypeOwner = serde_json::from_str(&metadata).unwrap();
    assert_eq!(persisted.resolution_state.as_deref(), Some("unresolved"));
}

#[test]
fn type_ownership_persisted_primary_templates_remain_separate_from_specializations() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "owner.cpp",
        "template<class T> class Owner {public: void run();}; template<class U> void Owner<U>::run() {} template<> class Owner<int> {public: void extra();}; void Owner<int>::extra() {}",
    )]);
    let (mut db, session) = database(&snapshot);
    finish(&mut db, &session);
    let primary = owner(&db, "owner.cpp", "run");
    let specialized = owner(&db, "owner.cpp", "extra");
    assert_eq!(primary.basis.as_deref(), Some("cpp_qualified"));
    assert_eq!(specialized.basis.as_deref(), Some("cpp_qualified"));
    assert_eq!(primary.resolution_state.as_deref(), Some("resolved"));
    assert_eq!(specialized.resolution_state.as_deref(), Some("resolved"));
    assert_ne!(primary.identity, specialized.identity);
}

#[test]
fn type_ownership_recovered_cpp_templates_preserve_parameter_and_argument_identity() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "owner.cpp",
        "template<class T> class API_EXPORT Owner {public: void run();}; template<class U> void Owner<U>::run() {} template<> class API_EXPORT Owner<int> {public: void extra();}; void Owner<int>::extra() {}",
    )]);
    let (mut db, session) = database(&snapshot);
    finish(&mut db, &session);
    let primary = owner(&db, "owner.cpp", "run");
    let specialized = owner(&db, "owner.cpp", "extra");
    assert_eq!(primary.resolution_state.as_deref(), Some("resolved"));
    assert_eq!(specialized.resolution_state.as_deref(), Some("resolved"));
    assert_ne!(primary.identity, specialized.identity);
    assert!(primary.target_hint.contains("<@0>"));
    assert!(specialized.target_hint.contains("<int>"));
}

#[test]
fn type_ownership_budget_charges_full_symbol_and_checkpoint_receipts() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        ("src/lib.rs", "mod model; mod actions;"),
        ("src/model.rs", "pub struct Owner;"),
        (
            "src/actions.rs",
            "use crate::model::Owner; impl Owner {fn run(){}}",
        ),
    ]);
    for receipt in [false, true] {
        let (mut db, mut session) = database(&snapshot);
        if receipt {
            db.execute(
                "UPDATE code_repository_index_checkpoints SET incremental_summary_json=?1",
                ["x".repeat(3072)],
            )
            .unwrap();
        } else {
            db.execute(
                "UPDATE code_repository_symbols SET doc_comment=?1 WHERE name='run'",
                ["x".repeat(4096)],
            )
            .unwrap();
        }
        session.resource_budget = CodeIndexResourceBudget::new(256, 4096, 64).unwrap();
        let mut rejected = false;
        for _ in 0..10 {
            let tx = db.transaction().unwrap();
            match advance(&tx, &session) {
                Err(StorageError::CapacityExceeded(_)) => {
                    tx.rollback().unwrap();
                    rejected = true;
                    break;
                }
                Ok(false) => tx.commit().unwrap(),
                other => panic!("oversized durable record was accepted: {other:?}"),
            }
        }
        assert!(rejected);
    }
}

#[test]
fn type_ownership_swift_module_roots_must_be_unique_even_without_same_named_types() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        (
            "packages/a/Sources/Models/Other.swift",
            "public struct Other {}",
        ),
        (
            "packages/b/Sources/Models/Owner.swift",
            "public struct Owner {}",
        ),
        (
            "Sources/App/Extra.swift",
            "import struct Models.Owner\nextension Owner {func run(){}}",
        ),
    ]);
    let (mut db, session) = database(&snapshot);
    finish(&mut db, &session);
    assert_eq!(
        owner(&db, "Sources/App/Extra.swift", "run")
            .resolution_state
            .as_deref(),
        Some("unresolved")
    );
}

#[test]
fn code_index_persistence_performance_suite_swift_module_proof_uses_language_index() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "Sources/Models/Owner.swift",
        "public struct Owner {}",
    )]);
    let (db, session) = database(&snapshot);
    db.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<16384) INSERT INTO code_repository_files(repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,is_generated,degraded_reason) SELECT 'repo',?1,printf('noise:%d',x),printf('noise/%d.rs',x),'rust','blob',1,1,'parsed',0,NULL FROM n",[&session.source_scope]).unwrap();
    let steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = steps.clone();
    db.progress_handler(
        1,
        Some(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            false
        }),
    );
    let mut bytes = 0;
    let root = crate::storage::sqlite::code::semantic_modules::swift_module_root(
        &db,
        &session.source_scope,
        "Models",
        &mut bytes,
        8192,
    )
    .unwrap();
    db.progress_handler(0, None::<fn() -> bool>);
    assert_eq!(root.as_deref(), Some("Sources/Models"));
    assert!(steps.load(std::sync::atomic::Ordering::Relaxed) < 5000);
    assert!(bytes < 1024);
}

#[test]
fn type_ownership_keeps_binary_and_test_crates_separate_from_the_library() {
    for (root, model) in [
        ("src/bin/app.rs", "src/bin/model.rs"),
        ("tests/check.rs", "tests/model.rs"),
        ("examples/demo.rs", "examples/model.rs"),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[
            ("src/lib.rs", "mod model;"),
            ("src/model.rs", "pub struct Owner;"),
            (
                root,
                "mod model; use crate::model::Owner; impl Owner {fn run(){}}",
            ),
            (model, "pub struct Owner;"),
        ]);
        let (mut db, session) = database(&snapshot);
        finish(&mut db, &session);
        let implementation = owner(&db, root, "run");
        assert_eq!(
            implementation.resolution_state.as_deref(),
            Some("resolved"),
            "{root}: {implementation:?}"
        );
        assert_eq!(implementation.identity, owner(&db, model, "Owner").identity);
        assert_ne!(
            implementation.identity,
            owner(&db, "src/model.rs", "Owner").identity
        );
    }
}

#[test]
fn type_ownership_requires_origin_membership_and_rejects_shared_crate_ambiguity() {
    for root in ["mod model;", "mod model; mod actions;"] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[
            ("src/lib.rs", root),
            ("src/main.rs", root),
            ("src/model.rs", "pub struct Owner;"),
            (
                "src/actions.rs",
                "use crate::model::Owner; impl Owner {fn run(){}}",
            ),
        ]);
        let (mut db, session) = database(&snapshot);
        finish(&mut db, &session);
        assert_eq!(
            owner(&db, "src/actions.rs", "run")
                .resolution_state
                .as_deref(),
            Some("unresolved")
        );
    }
}

#[test]
fn type_ownership_candidate_bytes_allow_prefix_pages_to_advance() {
    let methods = (0..40)
        .map(|i| format!("fn member_{i}(){{}}"))
        .collect::<String>();
    let implementation = format!("use crate::model::Owner; impl Owner {{{methods}}}");
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        ("src/lib.rs", "mod model; mod actions;"),
        ("src/model.rs", "pub struct Owner;"),
        ("src/actions.rs", &implementation),
    ]);
    let (mut db, mut session) = database(&snapshot);
    session.resource_budget = CodeIndexResourceBudget::new(256, 8192, 4).unwrap();
    let pages = finish(&mut db, &session);
    assert!(pages > 2);
    for i in 0..40 {
        assert_eq!(
            owner(&db, "src/actions.rs", &format!("member_{i}")).identity,
            owner(&db, "src/model.rs", "Owner").identity
        );
    }
}

#[test]
fn type_ownership_follows_module_redirection_and_import_aliases() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        ("src/lib.rs", "#[path=\"real.rs\"] mod model; mod actions;"),
        ("src/real.rs", "pub struct Owner;"),
        ("src/model.rs", "pub struct Owner;"),
        (
            "src/actions.rs",
            "use crate::model::Owner as Alias; impl Alias {fn run(){}}",
        ),
    ]);
    let (mut db, session) = database(&snapshot);
    finish(&mut db, &session);
    let implementation = owner(&db, "src/actions.rs", "run");
    assert_eq!(implementation.resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        implementation.identity,
        owner(&db, "src/real.rs", "Owner").identity
    );
    assert_ne!(
        implementation.identity,
        owner(&db, "src/model.rs", "Owner").identity
    );
}

#[test]
fn type_ownership_atomic_fallback_rejects_more_than_one_quantum() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "owner.rs",
        "struct Owner; impl Owner {fn a(){} fn b(){} fn c(){} }",
    )]);
    let (mut db, session) = database(&snapshot);
    let tx = db.transaction().unwrap();
    assert!(matches!(
        advance_atomically(&tx, &session),
        Err(StorageError::CapacityExceeded(_))
    ));
    tx.rollback().unwrap();
    let cursor: Option<String> = db
        .query_row(
            "SELECT type_owner_cursor FROM code_repository_index_checkpoints",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cursor, None);
}

fn database(snapshot: &CodeIndexSnapshot) -> (Connection, CodeIndexSession) {
    let mut db = Connection::open_in_memory().unwrap();
    crate::storage::sqlite::schema::initialization::initialize_schema(&db).unwrap();
    crate::storage::sqlite::code::schema::ensure_code_query_indexes(&db).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let session = CodeIndexSession {
        repository_id: "repo".into(),
        source_scope: snapshot.source_scope.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".into(),
        tree_hash: "tree".into(),
        path_filters: vec![],
        language_filters: vec![],
        full_replace: true,
        total_path_count: snapshot.files.len(),
        changed_path_count: snapshot.files.len(),
        skipped_unchanged_count: 0,
        deleted_paths: vec![],
        changed_paths: vec![],
        tombstones: vec![],
        workspaces: vec![],
        resource_budget: CodeIndexResourceBudget::new(1, 1024 * 1024, 4).unwrap(),
    };
    let tx = db.transaction().unwrap();
    super::super::super::checkpoint::insert(
        &tx,
        &session,
        super::super::phases::RESOLVE_IMPORTS,
        None,
    )
    .unwrap();
    crate::storage::sqlite::code::symbols::insert_records(&tx, &snapshot.symbols).unwrap();
    for import in &snapshot.imports {
        tx.execute(
            "INSERT INTO code_repository_imports (repository_id,source_scope,import_id,file_id,path,module,target_hint,resolution_state,confidence_basis_points,confidence_tier,line_start,line_end)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![import.repository_id,import.source_scope,import.import_id,import.file_id,import.path,import.module,import.target_hint,import.resolution_state,import.confidence_basis_points,import.confidence_tier,import.line_range.start,import.line_range.end],
        ).unwrap();
    }
    for file in &snapshot.files {
        tx.execute("INSERT INTO code_repository_files (repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,is_generated,degraded_reason) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![file.repository_id,file.source_scope,file.file_id,file.path,file.language_id,file.blob_hash,file.byte_len,file.line_count,file.parse_status.as_str(),file.is_generated,file.degraded_reason]).unwrap();
    }
    tx.commit().unwrap();
    (db, session)
}

#[test]
fn type_ownership_cpp_recovered_containers_use_resolved_conditional_includes() {
    for wrapper in ["API_BEGIN", "OTHER_BEGIN"] {
        let implementation = format!(
            "#ifndef HEADER_ONLY\n#include <pkg/owner.h>\n#endif\n{wrapper}\nnamespace detail {{\nAPI_INLINE bool Owner::ready() const {{ return flag.load(); }}\n}}\nAPI_END"
        );
        let snapshot = crate::code::syntax_snapshot_for_tests(&[
            (
                "include/pkg/owner.h",
                "API_BEGIN\nnamespace detail {\nclass API_EXPORT Owner { public: bool ready() const; };\n}\nAPI_END",
            ),
            ("include/pkg/owner-inl.h", &implementation),
        ]);
        assert!(
            !snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "class" && symbol.name == "API_EXPORT")
        );
        assert!(
            snapshot
                .imports
                .iter()
                .any(|import| import.resolution_state == "resolved"
                    && import.target_hint.as_deref() == Some("include/pkg/owner.h"))
        );
        let (mut db, session) = database(&snapshot);
        finish(&mut db, &session);
        let declaration = owner(&db, "include/pkg/owner.h", "Owner");
        let member = owner(&db, "include/pkg/owner-inl.h", "ready");
        assert_eq!(
            member.resolution_state.as_deref(),
            Some(if wrapper == "API_BEGIN" {
                "resolved"
            } else {
                "unresolved"
            })
        );
        if wrapper == "API_BEGIN" {
            assert_eq!(member.identity, declaration.identity);
            assert_eq!(member.target_paths, ["include/pkg/owner.h"]);
            db.execute(
                "UPDATE code_repository_imports SET resolution_state='ambiguous',target_hint=NULL",
                [],
            )
            .unwrap();
            db.execute(
                "UPDATE code_repository_index_checkpoints SET type_owner_cursor=NULL",
                [],
            )
            .unwrap();
            finish(&mut db, &session);
            let replayed = owner(&db, "include/pkg/owner-inl.h", "ready");
            assert_eq!(replayed.resolution_state.as_deref(), Some("unresolved"));
            assert!(replayed.target_paths.is_empty());
        }
    }
}

#[test]
fn type_ownership_cpp_duplicate_imported_types_remain_ambiguous() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        ("include/a.h", "class Owner { public: void run(); };"),
        ("include/b.h", "class Owner { public: void run(); };"),
        (
            "app.cpp",
            "#if ENABLED\n#include <a.h>\n#include <b.h>\n#endif\nvoid Owner::run() {}",
        ),
    ]);
    let (mut db, session) = database(&snapshot);
    finish(&mut db, &session);
    assert_eq!(
        owner(&db, "app.cpp", "run").resolution_state.as_deref(),
        Some("ambiguous")
    );
}

fn finish(db: &mut Connection, session: &CodeIndexSession) -> usize {
    for page in 1..100 {
        let tx = db.transaction().unwrap();
        let complete = advance(&tx, session).unwrap();
        tx.commit().unwrap();
        if complete {
            return page;
        }
    }
    panic!("ownership pages did not terminate");
}

fn owner(db: &Connection, path: &str, name: &str) -> CodeTypeOwner {
    let metadata: String = db
        .query_row(
            "SELECT type_owner_json FROM code_repository_symbols WHERE path=?1 AND name=?2 AND type_owner_json IS NOT NULL",
            params![path, name],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&metadata).unwrap()
}

#[test]
fn type_ownership_pages_resolve_explicit_cross_file_implementations() {
    for (declaration_path, declaration, implementation_path, implementation) in [
        (
            "Sources/Models/Owner.swift",
            "public struct Owner {}",
            "Sources/App/Extras.swift",
            "import struct Models.Owner\nextension Owner {\n func run() {}\n}\n",
        ),
        (
            "src/model.rs",
            "pub struct Owner;",
            "src/actions.rs",
            "use crate::model::Owner; impl Owner { fn run() {} }",
        ),
        (
            "src/model.hpp",
            "namespace demo { class Owner { public: void run(); }; }",
            "src/actions.cpp",
            "#include \"model.hpp\"\nvoid demo::Owner::run() {}",
        ),
        (
            "pkg/model.go",
            "package demo\ntype Owner struct {}",
            "pkg/actions.go",
            "package demo\nfunc (o *Owner) run() {}",
        ),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[
            ("src/lib.rs", "mod model; mod actions;"),
            (declaration_path, declaration),
            (implementation_path, implementation),
        ]);
        let (mut db, session) = database(&snapshot);
        finish(&mut db, &session);
        let declaration = owner(&db, declaration_path, "Owner");
        let implementation = owner(&db, implementation_path, "run");
        assert_eq!(
            implementation.resolution_state.as_deref(),
            Some("resolved"),
            "{implementation_path}: {implementation:?}"
        );
        assert_eq!(implementation.identity, declaration.identity);
    }
}

#[test]
fn type_ownership_replay_rolls_back_cursor_and_recomputes_deleted_targets() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        ("src/lib.rs", "mod model; mod actions;"),
        ("src/model.rs", "pub struct Owner;"),
        (
            "src/actions.rs",
            "use crate::model::Owner; impl Owner { fn run() {} fn extra() {} }",
        ),
    ]);
    let (mut db, session) = database(&snapshot);
    let tx = db.transaction().unwrap();
    assert!(!advance(&tx, &session).unwrap());
    tx.rollback().unwrap();
    let cursor: Option<String> = db
        .query_row(
            "SELECT type_owner_cursor FROM code_repository_index_checkpoints",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cursor, None);
    assert!(finish(&mut db, &session) >= 3);
    assert_eq!(
        owner(&db, "src/actions.rs", "run")
            .resolution_state
            .as_deref(),
        Some("resolved")
    );
    db.execute(
        "DELETE FROM code_repository_symbols WHERE path='src/model.rs'",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE code_repository_index_checkpoints SET type_owner_cursor=NULL",
        [],
    )
    .unwrap();
    finish(&mut db, &session);
    assert_eq!(
        owner(&db, "src/actions.rs", "run")
            .resolution_state
            .as_deref(),
        Some("unresolved")
    );
}

#[test]
fn type_ownership_does_not_join_unimported_or_ambiguous_types() {
    for imports in ["", "use crate::model::Owner;"] {
        let implementation = format!("{imports} impl Owner {{ fn run() {{}} }}");
        let snapshot = crate::code::syntax_snapshot_for_tests(&[
            ("src/lib.rs", "mod model; mod actions;"),
            ("src/model.rs", "pub struct Owner;"),
            ("src/model/mod.rs", "pub struct Owner;"),
            ("src/actions.rs", &implementation),
        ]);
        let (mut db, session) = database(&snapshot);
        finish(&mut db, &session);
        assert_eq!(
            owner(&db, "src/actions.rs", "run")
                .resolution_state
                .as_deref(),
            Some(if imports.is_empty() {
                "unresolved"
            } else {
                "ambiguous"
            })
        );
    }
}
