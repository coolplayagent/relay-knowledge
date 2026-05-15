use crate::domain::RepositoryCodeRange;

pub(super) struct SymbolRow {
    pub(super) symbol_snapshot_id: String,
    pub(super) canonical_symbol_id: String,
    pub(super) file_id: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) signature: String,
    pub(super) doc_comment: Option<String>,
    pub(super) byte_range: RepositoryCodeRange,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) name: String,
    pub(super) qualified_name: String,
    pub(super) kind: String,
}

pub(super) struct ReferenceRow {
    pub(super) file_id: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) target_symbol_snapshot_id: Option<String>,
    pub(super) byte_range: RepositoryCodeRange,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) target_hint: Option<String>,
    pub(super) resolution_state: String,
    pub(super) confidence_basis_points: u16,
    pub(super) confidence_tier: String,
    pub(super) target_canonical_symbol_id: Option<String>,
}

pub(super) struct CallRow {
    pub(super) file_id: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) caller_symbol_snapshot_id: Option<String>,
    pub(super) caller_name: Option<String>,
    pub(super) callee_symbol_snapshot_id: Option<String>,
    pub(super) callee_name: String,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) target_hint: Option<String>,
    pub(super) resolution_state: String,
    pub(super) confidence_basis_points: u16,
    pub(super) confidence_tier: String,
    pub(super) caller_canonical_symbol_id: Option<String>,
    pub(super) callee_canonical_symbol_id: Option<String>,
    pub(super) caller_excerpt: Option<String>,
}

pub(super) struct ImportRow {
    pub(super) file_id: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) module: String,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) target_hint: Option<String>,
    pub(super) resolution_state: String,
    pub(super) confidence_basis_points: u16,
    pub(super) confidence_tier: String,
}

pub(super) struct ChunkRow {
    pub(super) file_id: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) content: String,
    pub(super) byte_range: RepositoryCodeRange,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) symbol_snapshot_id: Option<String>,
    pub(super) canonical_symbol_id: Option<String>,
    pub(super) parse_status: String,
    pub(super) degraded_reason: Option<String>,
}
