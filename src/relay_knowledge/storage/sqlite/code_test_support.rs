use crate::domain::{
    CodeCallRecord, CodeImportRecord, CodeParseStatus, RepositoryCodeChunkRecord,
    RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord,
};

pub(super) const TEST_SOURCE_SCOPE: &str = "git_snapshot:test";

pub(super) fn file(
    file_id: &str,
    path: &str,
    language_id: &str,
    parse_status: CodeParseStatus,
    degraded_reason: Option<String>,
) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("{file_id}-hash"),
        byte_len: 20,
        line_count: 1,
        parse_status,
        degraded_reason,
    }
}

pub(super) fn symbol(
    id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 8 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

pub(super) fn reference(
    id: &str,
    file_id: &str,
    path: &str,
    target_symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        reference_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: "target".to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: target_symbol_snapshot_id.map(str::to_owned),
        target_hint: Some("target".to_owned()),
        resolution_state: if target_symbol_snapshot_id.is_some() {
            "resolved".to_owned()
        } else {
            "unresolved".to_owned()
        },
        confidence_basis_points: if target_symbol_snapshot_id.is_some() {
            8_000
        } else {
            2_500
        },
        confidence_tier: if target_symbol_snapshot_id.is_some() {
            "inferred".to_owned()
        } else {
            "ambiguous".to_owned()
        },
        byte_range: RepositoryCodeRange { start: 0, end: 6 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

pub(super) fn import(id: &str, file_id: &str, path: &str) -> CodeImportRecord {
    import_module(id, file_id, path, "module::target")
}

pub(super) fn import_module(id: &str, file_id: &str, path: &str, module: &str) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        import_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: Some(module.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 10_000,
        confidence_tier: "extracted".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

pub(super) fn chunk(
    id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        content: content.to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 20 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
    }
}

pub(super) fn call(
    id: &str,
    file_id: &str,
    path: &str,
    callee_symbol_snapshot_id: Option<&str>,
) -> CodeCallRecord {
    CodeCallRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        call_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        caller_symbol_snapshot_id: None,
        caller_name: Some("caller".to_owned()),
        callee_symbol_snapshot_id: callee_symbol_snapshot_id.map(str::to_owned),
        callee_name: "target".to_owned(),
        target_hint: Some("target".to_owned()),
        resolution_state: if callee_symbol_snapshot_id.is_some() {
            "resolved".to_owned()
        } else {
            "unresolved".to_owned()
        },
        confidence_basis_points: if callee_symbol_snapshot_id.is_some() {
            8_000
        } else {
            2_500
        },
        confidence_tier: if callee_symbol_snapshot_id.is_some() {
            "inferred".to_owned()
        } else {
            "ambiguous".to_owned()
        },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}
