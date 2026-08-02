use crate::domain::{
    CodeImportRecord, CodeParseStatus, RepositoryCodeFileRecord, RepositoryCodeRange,
    RepositoryCodeSymbolRecord,
};

pub(super) fn file(path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: "git_snapshot:test".to_owned(),
        file_id: format!("file:{}", path.replace('/', ":")),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash:{}", path.replace('/', ":")),
        byte_len: 32,
        line_count: 3,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

pub(super) fn symbol(
    path: &str,
    language_id: &str,
    name: &str,
    qualified_name: &str,
    kind: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: "git_snapshot:test".to_owned(),
        symbol_snapshot_id: format!("snapshot:{qualified_name}"),
        canonical_symbol_id: format!("canonical:{qualified_name}"),
        file_id: format!("file:{}", path.replace('/', ":")),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        qualified_name: qualified_name.to_owned(),
        kind: kind.to_owned(),
        signature: format!("{kind} {name}"),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 8 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_role: None,
    }
}

pub(super) fn import() -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: "git_snapshot:test".to_owned(),
        import_id: "import:test".to_owned(),
        file_id: "file:src:main.rs".to_owned(),
        path: "src/main.rs".to_owned(),
        module: "crate::client".to_owned(),
        target_hint: None,
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 0,
        confidence_tier: "unknown".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}
