//! Ownership pages obey the same publication barrier as reference pages.
use super::*;

#[tokio::test]
async fn type_ownership_pages_resume_after_reopen_and_query_index_repair() {
    let (store, session, fence) = ownership_session().await;
    let first = store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .unwrap();
    let crate::storage::CodeIndexFinalizationStep::Pending { checkpoint_state } = first else {
        panic!("first ownership page must remain pending");
    };
    assert!(checkpoint_state.starts_with("finalizing:resolve_type_ownership:"));
    let checkpoint = store
        .code_index_checkpoint(session.source_scope.clone())
        .await
        .unwrap()
        .unwrap();
    store
        .begin_code_index_session_at_checkpoint_with_fence(
            session.clone(),
            Some(checkpoint),
            fence.clone(),
        )
        .await
        .unwrap();
    store
        .run(|db| {
            db.execute("DROP INDEX code_repository_symbols_type_owner_lookup", [])?;
            Ok(())
        })
        .await
        .unwrap();
    let mut resumed_page = false;
    for _ in 0..30 {
        let step = store
            .advance_code_index_session_with_fence(session.clone(), fence.clone())
            .await
            .unwrap();
        let crate::storage::CodeIndexFinalizationStep::Pending {
            checkpoint_state: next,
        } = step
        else {
            panic!("call-target checkpoint should be observed before publication");
        };
        if next.starts_with("finalizing:resolve_type_ownership:") {
            assert_ne!(next, checkpoint_state);
            resumed_page = true;
        }
        if next == "finalizing:resolve_call_targets" {
            assert!(resumed_page);
            return;
        }
    }
    panic!("ownership repair did not resume within the phase budget");
}

#[tokio::test]
async fn type_ownership_corrupt_cursor_is_rejected_before_query_index_repair() {
    let (store, session, fence) = ownership_session().await;
    store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .unwrap();
    store
        .run(|db| {
            db.execute(
                "UPDATE code_repository_index_checkpoints SET type_owner_cursor='wrong'",
                [],
            )?;
            db.execute("DROP INDEX code_repository_symbols_type_owner_lookup", [])?;
            Ok(())
        })
        .await
        .unwrap();
    let before = store
        .code_index_checkpoint(session.source_scope.clone())
        .await
        .unwrap()
        .unwrap();
    let error = store
        .advance_code_index_session_with_fence(session.clone(), fence)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not match its durable cursor"),
        "{error}"
    );
    let after = store
        .code_index_checkpoint(session.source_scope)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
    assert!(!store.run(|db| {
        Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='code_repository_symbols_type_owner_lookup')", [], |r| r.get::<_,bool>(0))?)
    }).await.unwrap());
}

async fn ownership_session() -> (
    crate::storage::SqliteGraphStore,
    crate::domain::CodeIndexSession,
    CodeIndexPublicationFence,
) {
    let store = registered_store().await;
    let scope = "git_snapshot:ownership-resume";
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 4).unwrap();
    let (mut session, fence) = begin_fenced_session(
        &store,
        scope,
        "ownership-resume",
        "ownership-worker",
        budget,
    )
    .await;
    session.total_path_count = 3;
    session.changed_path_count = 3;
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .unwrap();
    for ordinal in 1..=3 {
        let path = format!("file{ordinal}.rs");
        let id = format!("symbol:{ordinal}");
        let name = format!("Owner{ordinal}");
        let mut declaration = super::super::tests::symbol(scope, &id, &path, &path, &name, "rust");
        declaration.kind = "struct".into();
        declaration.signature = format!("struct Owner{ordinal};");
        declaration.type_owner = Some(crate::domain::CodeTypeOwner {
            identity: format!("rust|{path}|Owner{ordinal}"),
            relation: "declaration".into(),
            target_hint: format!("Owner{ordinal}"),
            lookup_identity: None,
            basis: Some("lexical".into()),
            resolution_state: Some("resolved".into()),
            import_target: None,
            visibility: None,
            target_paths: Vec::new(),
        });
        store
            .apply_code_index_batch_with_fence(
                crate::domain::CodeIndexBatch {
                    files: vec![file(scope, &path, &path, "rust", CodeParseStatus::Parsed)],
                    symbols: vec![declaration],
                    ..batch(scope, ordinal)
                },
                fence.clone(),
            )
            .await
            .unwrap();
    }
    store
        .run(|db| {
            super::super::super::super::schema::ensure_code_query_indexes(db)?;
            db.execute(
                "UPDATE code_repository_index_checkpoints SET state='finalizing:resolve_imports'",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    (store, session, fence)
}

#[tokio::test]
async fn active_scope_type_ownership_page_is_rejected_without_cursor_mutation() {
    let store = registered_store().await;
    let scope = "git_snapshot:active-ownership-page";
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 8).unwrap();
    let (session, fence) = begin_fenced_session(
        &store,
        scope,
        "active-ownership-page",
        "ownership-worker",
        budget,
    )
    .await;
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .unwrap();
    store
        .apply_code_index_batch_with_fence(
            crate::domain::CodeIndexBatch {
                files: vec![file(
                    scope,
                    "file-1",
                    "owner.rs",
                    "rust",
                    CodeParseStatus::Parsed,
                )],
                ..batch(scope, 1)
            },
            fence.clone(),
        )
        .await
        .unwrap();
    store.run(move |db|{
        super::super::super::super::schema::ensure_code_query_indexes(db)?;
        db.execute("UPDATE code_repository_index_checkpoints SET state='finalizing:resolve_imports' WHERE source_scope=?1",[scope])?;
        db.execute("UPDATE code_repositories SET last_indexed_scope_id=?1 WHERE repository_id='repo'",[scope])?;
        Ok(())
    }).await.unwrap();
    let error = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("cannot mutate queryable scope"),
        "{error}"
    );
    let (state,cursor)=store.run(move |db|{
        Ok(db.query_row("SELECT state,type_owner_cursor FROM code_repository_index_checkpoints WHERE source_scope=?1",[scope],|row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?)))?)
    }).await.unwrap();
    assert_eq!(state, "finalizing:resolve_imports");
    assert_eq!(cursor, None);
}
