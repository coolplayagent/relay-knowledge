use super::*;

#[tokio::test]
async fn empty_filtered_incremental_delta_publishes_clone_without_edge_work() {
    let store = registered_store().await;
    let base_scope = "git_snapshot:empty-delta-base";
    let base_session = session_for_scope(base_scope, 1);
    store
        .begin_code_index_session(base_session.clone())
        .await
        .expect("base session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                base_scope,
                "stable-file",
                "src/stable.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            symbols: vec![symbol(
                base_scope,
                "stable-symbol",
                "stable-file",
                "src/stable.rs",
                "stable",
                "rust",
            )],
            references: vec![reference(
                base_scope,
                "stable-reference",
                "stable-file",
                "src/stable.rs",
                "stable",
            )],
            ..batch(base_scope, 1)
        })
        .await
        .expect("base batch should persist");
    store
        .finalize_code_index_session(base_session)
        .await
        .expect("base session should finalize");

    let target_scope = "git_snapshot:empty-delta-target";
    let mut incremental = session_for_scope(target_scope, 0);
    incremental.base_resolved_commit_sha = Some("commit".to_owned());
    incremental.resolved_commit_sha = "commit-2".to_owned();
    incremental.tree_hash = "tree-2".to_owned();
    incremental.full_replace = false;
    incremental.skipped_unchanged_count = 1;
    store
        .begin_code_index_session(incremental.clone())
        .await
        .expect("empty incremental session should clone its base");
    let summary = store
        .finalize_code_index_session(incremental)
        .await
        .expect("empty incremental session should publish");

    assert_eq!(summary.changed_path_count, 0);
    assert_eq!(summary.progress.parsed_file_count, 0);
    assert_eq!(
        reference_resolution_rows(&store, target_scope).await,
        reference_resolution_rows(&store, base_scope).await
    );
}

#[tokio::test]
async fn incremental_symbol_move_refinalizes_unchanged_module_import() {
    let store = registered_store().await;
    let base_scope = "git_snapshot:symbol-move-base";
    let base_session = session_for_scope(base_scope, 8);
    let mut base_files = vec![
        file(
            base_scope,
            "module-a-file",
            "src/module-a.ts",
            "typescript",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "module-b-file",
            "src/module-b.ts",
            "typescript",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "consumer-file",
            "src/consumer.ts",
            "typescript",
            CodeParseStatus::Parsed,
        ),
    ];
    for index in 1..=5 {
        base_files.push(file(
            base_scope,
            &format!("extra-{index}-file"),
            &format!("src/extra-{index}.ts"),
            "typescript",
            CodeParseStatus::Parsed,
        ));
    }
    store
        .begin_code_index_session(base_session.clone())
        .await
        .expect("base session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: base_files,
            symbols: vec![symbol(
                base_scope,
                "moved-symbol",
                "module-a-file",
                "src/module-a.ts",
                "Moved",
                "typescript",
            )],
            imports: vec![import(
                base_scope,
                "moved-import",
                "consumer-file",
                "src/consumer.ts",
                "import { Moved as LocalMoved } from './module-b';",
            )],
            ..batch(base_scope, 1)
        })
        .await
        .expect("base batch should persist");
    store
        .finalize_code_index_session(base_session)
        .await
        .expect("base session should finalize");
    assert_eq!(
        import_resolution_state(&store, base_scope, "moved-import").await,
        "unresolved"
    );

    let target_scope = "git_snapshot:symbol-move-target";
    let mut incremental = session_for_scope(target_scope, 8);
    incremental.base_resolved_commit_sha = Some("commit".to_owned());
    incremental.resolved_commit_sha = "commit-2".to_owned();
    incremental.tree_hash = "tree-2".to_owned();
    incremental.full_replace = false;
    incremental.changed_path_count = 2;
    incremental.skipped_unchanged_count = 6;
    incremental.changed_paths = vec!["src/module-a.ts".to_owned(), "src/module-b.ts".to_owned()];
    store
        .begin_code_index_session(incremental.clone())
        .await
        .expect("incremental session should clone its base");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![
                file(
                    target_scope,
                    "module-a-file",
                    "src/module-a.ts",
                    "typescript",
                    CodeParseStatus::Parsed,
                ),
                file(
                    target_scope,
                    "module-b-file",
                    "src/module-b.ts",
                    "typescript",
                    CodeParseStatus::Parsed,
                ),
            ],
            symbols: vec![symbol(
                target_scope,
                "moved-symbol",
                "module-b-file",
                "src/module-b.ts",
                "Moved",
                "typescript",
            )],
            ..batch(target_scope, 1)
        })
        .await
        .expect("incremental batch should persist");
    store
        .finalize_code_index_session(incremental)
        .await
        .expect("incremental session should finalize");

    assert_eq!(
        import_resolution_state(&store, target_scope, "moved-import").await,
        "resolved"
    );
}

#[tokio::test]
async fn incremental_symbol_cardinality_change_refinalizes_unchanged_calls() {
    let store = registered_store().await;
    let base_scope = "git_snapshot:cardinality-base";
    let base_session = session_for_scope(base_scope, 5);
    let base_files = vec![
        file(
            base_scope,
            "target-a-file",
            "src/target-a.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "consumer-file",
            "src/consumer.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "extra-1-file",
            "src/extra-1.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "extra-2-file",
            "src/extra-2.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "extra-3-file",
            "src/extra-3.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
    ];
    store
        .begin_code_index_session(base_session.clone())
        .await
        .expect("base session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: base_files,
            symbols: vec![symbol(
                base_scope,
                "shared-a",
                "target-a-file",
                "src/target-a.rs",
                "shared",
                "rust",
            )],
            references: vec![reference(
                base_scope,
                "shared-call",
                "consumer-file",
                "src/consumer.rs",
                "shared",
            )],
            ..batch(base_scope, 1)
        })
        .await
        .expect("base batch should persist");
    store
        .finalize_code_index_session(base_session)
        .await
        .expect("base session should finalize");

    let target_scope = "git_snapshot:cardinality-target";
    let mut incremental = session_for_scope(target_scope, 6);
    incremental.base_resolved_commit_sha = Some("commit".to_owned());
    incremental.resolved_commit_sha = "commit-2".to_owned();
    incremental.tree_hash = "tree-2".to_owned();
    incremental.full_replace = false;
    incremental.changed_path_count = 1;
    incremental.skipped_unchanged_count = 5;
    incremental.changed_paths = vec!["src/target-b.rs".to_owned()];
    store
        .begin_code_index_session(incremental.clone())
        .await
        .expect("incremental session should clone its base");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                target_scope,
                "target-b-file",
                "src/target-b.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            symbols: vec![symbol(
                target_scope,
                "shared-b",
                "target-b-file",
                "src/target-b.rs",
                "shared",
                "rust",
            )],
            ..batch(target_scope, 1)
        })
        .await
        .expect("incremental batch should persist");
    store
        .finalize_code_index_session(incremental)
        .await
        .expect("incremental session should finalize");

    let references = reference_resolution_rows(&store, target_scope).await;
    assert_eq!(
        references.get("shared-call"),
        Some(&("ambiguous".to_owned(), None, 5_000, "ambiguous".to_owned()))
    );
    let call_resolution = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT resolution_state, callee_symbol_snapshot_id
                     FROM code_repository_calls
                     WHERE source_scope = ?1 AND path = 'src/consumer.rs'",
                    [target_scope],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("call resolution should load");
    assert_eq!(call_resolution, ("ambiguous".to_owned(), None));
}

#[tokio::test]
async fn incremental_symbol_change_refinalizes_unchanged_named_import_aliases() {
    let store = registered_store().await;
    let base_scope = "git_snapshot:named-import-base";
    let base_session = session_for_scope(base_scope, 10);
    let mut base_files = vec![
        file(
            base_scope,
            "target-a-file",
            "src/target-a.ts",
            "typescript",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "target-b-file",
            "src/target-b.ts",
            "typescript",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "consumer-file",
            "src/consumer.ts",
            "typescript",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "python-consumer-file",
            "src/consumer.py",
            "python",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "python-target-file",
            "src/target_a.py",
            "python",
            CodeParseStatus::Parsed,
        ),
    ];
    for index in 1..=5 {
        base_files.push(file(
            base_scope,
            &format!("extra-{index}-file"),
            &format!("src/extra-{index}.ts"),
            "typescript",
            CodeParseStatus::Parsed,
        ));
    }
    store
        .begin_code_index_session(base_session.clone())
        .await
        .expect("base session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: base_files,
            symbols: vec![
                symbol(
                    base_scope,
                    "shared-a",
                    "target-a-file",
                    "src/target-a.ts",
                    "Shared",
                    "typescript",
                ),
                symbol(
                    base_scope,
                    "python-shared-a",
                    "python-target-file",
                    "src/target_a.py",
                    "Shared",
                    "python",
                ),
            ],
            imports: vec![
                import(
                    base_scope,
                    "shared-import",
                    "consumer-file",
                    "src/consumer.ts",
                    "import { Shared as LocalShared } from './target-a';",
                ),
                import(
                    base_scope,
                    "python-shared-import",
                    "python-consumer-file",
                    "src/consumer.py",
                    "from target_a import Shared as LocalShared",
                ),
            ],
            references: vec![
                reference(
                    base_scope,
                    "shared-alias-call",
                    "consumer-file",
                    "src/consumer.ts",
                    "LocalShared",
                ),
                reference(
                    base_scope,
                    "python-shared-alias-call",
                    "python-consumer-file",
                    "src/consumer.py",
                    "LocalShared",
                ),
            ],
            ..batch(base_scope, 1)
        })
        .await
        .expect("base batch should persist");
    store
        .finalize_code_index_session(base_session)
        .await
        .expect("base session should finalize");

    let target_scope = "git_snapshot:named-import-target";
    let mut incremental = session_for_scope(target_scope, 10);
    incremental.base_resolved_commit_sha = Some("commit".to_owned());
    incremental.resolved_commit_sha = "commit-2".to_owned();
    incremental.tree_hash = "tree-2".to_owned();
    incremental.full_replace = false;
    incremental.changed_path_count = 2;
    incremental.skipped_unchanged_count = 8;
    incremental.changed_paths = vec!["src/target-a.ts".to_owned(), "src/target_a.py".to_owned()];
    store
        .begin_code_index_session(incremental.clone())
        .await
        .expect("incremental session should clone its base");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![
                file(
                    target_scope,
                    "target-a-file",
                    "src/target-a.ts",
                    "typescript",
                    CodeParseStatus::Parsed,
                ),
                file(
                    target_scope,
                    "python-target-file",
                    "src/target_a.py",
                    "python",
                    CodeParseStatus::Parsed,
                ),
            ],
            symbols: vec![
                symbol(
                    target_scope,
                    "replacement-a",
                    "target-a-file",
                    "src/target-a.ts",
                    "Replacement",
                    "typescript",
                ),
                symbol(
                    target_scope,
                    "python-replacement-a",
                    "python-target-file",
                    "src/target_a.py",
                    "Replacement",
                    "python",
                ),
            ],
            ..batch(target_scope, 1)
        })
        .await
        .expect("incremental batch should persist");
    store
        .finalize_code_index_session(incremental)
        .await
        .expect("incremental session should finalize");

    let references = reference_resolution_rows(&store, target_scope).await;
    assert_eq!(
        references.get("shared-alias-call"),
        Some(&("unresolved".to_owned(), None, 2_500, "ambiguous".to_owned()))
    );
    assert_eq!(
        references.get("python-shared-alias-call"),
        Some(&("unresolved".to_owned(), None, 2_500, "ambiguous".to_owned()))
    );
}

#[tokio::test]
async fn incremental_callable_metadata_change_refinalizes_unchanged_calls() {
    let store = registered_store().await;
    let base_scope = "git_snapshot:callable-metadata-base";
    let base_session = session_for_scope(base_scope, 7);
    let mut base_files = vec![
        file(
            base_scope,
            "definition-a-file",
            "src/definition-a.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "candidate-b-file",
            "src/candidate-b.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
        file(
            base_scope,
            "consumer-file",
            "src/consumer.rs",
            "rust",
            CodeParseStatus::Parsed,
        ),
    ];
    for index in 1..=4 {
        base_files.push(file(
            base_scope,
            &format!("extra-{index}-file"),
            &format!("src/extra-{index}.rs"),
            "rust",
            CodeParseStatus::Parsed,
        ));
    }
    let definition_a = symbol(
        base_scope,
        "stable-a",
        "definition-a-file",
        "src/definition-a.rs",
        "stable",
        "rust",
    );
    let mut declaration_b = symbol(
        base_scope,
        "stable-b",
        "candidate-b-file",
        "src/candidate-b.rs",
        "stable",
        "rust",
    );
    declaration_b.kind = "function_declaration".to_owned();
    declaration_b.signature = "fn stable();".to_owned();
    store
        .begin_code_index_session(base_session.clone())
        .await
        .expect("base session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: base_files,
            symbols: vec![definition_a, declaration_b],
            references: vec![reference(
                base_scope,
                "stable-call",
                "consumer-file",
                "src/consumer.rs",
                "stable",
            )],
            ..batch(base_scope, 1)
        })
        .await
        .expect("base batch should persist");
    store
        .finalize_code_index_session(base_session)
        .await
        .expect("base session should finalize");
    assert_eq!(
        reference_resolution_rows(&store, base_scope)
            .await
            .get("stable-call")
            .map(|row| row.0.as_str()),
        Some("resolved")
    );

    let target_scope = "git_snapshot:callable-metadata-target";
    let mut incremental = session_for_scope(target_scope, 7);
    incremental.base_resolved_commit_sha = Some("commit".to_owned());
    incremental.resolved_commit_sha = "commit-2".to_owned();
    incremental.tree_hash = "tree-2".to_owned();
    incremental.full_replace = false;
    incremental.changed_path_count = 1;
    incremental.skipped_unchanged_count = 6;
    incremental.changed_paths = vec!["src/candidate-b.rs".to_owned()];
    let definition_b = symbol(
        target_scope,
        "stable-b",
        "candidate-b-file",
        "src/candidate-b.rs",
        "stable",
        "rust",
    );
    store
        .begin_code_index_session(incremental.clone())
        .await
        .expect("incremental session should clone its base");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                target_scope,
                "candidate-b-file",
                "src/candidate-b.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            symbols: vec![definition_b],
            ..batch(target_scope, 1)
        })
        .await
        .expect("incremental batch should persist");
    store
        .finalize_code_index_session(incremental)
        .await
        .expect("incremental session should finalize");

    assert_eq!(
        reference_resolution_rows(&store, target_scope)
            .await
            .get("stable-call")
            .map(|row| row.0.as_str()),
        Some("ambiguous")
    );
}

#[tokio::test]
async fn incremental_module_addition_refinalizes_unchanged_side_effect_imports() {
    let store = registered_store().await;
    let base_scope = "git_snapshot:import-base";
    let base_session = session_for_scope(base_scope, 5);
    let mut base_files = vec![file(
        base_scope,
        "importer-file",
        "src/importer.ts",
        "typescript",
        CodeParseStatus::Parsed,
    )];
    for index in 1..=4 {
        base_files.push(file(
            base_scope,
            &format!("extra-{index}-file"),
            &format!("src/extra-{index}.ts"),
            "typescript",
            CodeParseStatus::Parsed,
        ));
    }
    store
        .begin_code_index_session(base_session.clone())
        .await
        .expect("base session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: base_files,
            imports: vec![import(
                base_scope,
                "side-effect-import",
                "importer-file",
                "src/importer.ts",
                "import './new_module';",
            )],
            ..batch(base_scope, 1)
        })
        .await
        .expect("base batch should persist");
    store
        .finalize_code_index_session(base_session)
        .await
        .expect("base session should finalize");
    assert_eq!(
        import_resolution_state(&store, base_scope, "side-effect-import").await,
        "unresolved"
    );

    let target_scope = "git_snapshot:import-target";
    let mut incremental = session_for_scope(target_scope, 6);
    incremental.base_resolved_commit_sha = Some("commit".to_owned());
    incremental.resolved_commit_sha = "commit-2".to_owned();
    incremental.tree_hash = "tree-2".to_owned();
    incremental.full_replace = false;
    incremental.changed_path_count = 1;
    incremental.skipped_unchanged_count = 5;
    incremental.changed_paths = vec!["src/new_module.ts".to_owned()];
    store
        .begin_code_index_session(incremental.clone())
        .await
        .expect("incremental session should clone its base");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                target_scope,
                "new-module-file",
                "src/new_module.ts",
                "typescript",
                CodeParseStatus::Parsed,
            )],
            ..batch(target_scope, 1)
        })
        .await
        .expect("incremental batch should persist");
    store
        .finalize_code_index_session(incremental)
        .await
        .expect("incremental session should finalize");

    assert_eq!(
        import_resolution_state(&store, target_scope, "side-effect-import").await,
        "resolved"
    );
}
