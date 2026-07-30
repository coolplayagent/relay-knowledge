mod multimodal;
mod mutation;
pub(super) mod retrieval;

use super::core::{DomainError, GraphVersion, SourceScope, error};

pub use multimodal::{
    EvidenceExtractionMetadata, EvidenceModality, ExtractionDiagnostic, ExtractionStatus,
    LayoutRegion,
};
pub use mutation::{
    ClaimRecord, CommitReceipt, ConfidenceScore, EventRecord, EvidenceRecord, EvidenceSpan,
    FactStatus, GraphMutationBatch, GraphRelationRecord, GraphVersionRange,
};
pub use retrieval::{
    CodeGraphArtifact, CodeGraphArtifactKind, ContextEntity, ContextGraphFact,
    ContextGraphFactKind, ContextGraphPath, ContextGraphPathEdge, ContextPackItem, FreshnessPolicy,
    FusionDiagnostics, RECIPROCAL_RANK_FUSION_K, RankingSignal, RerankDiagnostics, RerankMode,
    RerankModeError, RerankSignal, RetrievalBackendState, RetrievalBackendStatus,
    RetrievalBudgetUsed, RetrievalHit, RetrievalMode, RetrievedContextPack, RetrieverSource,
    TraversalProvenanceTrace, TraversalRankingContribution, TraversalTraceEdge,
    TraversalTraceEvidence, TraversalTraceNode, TraversalTraceNodeKind, TraversalTraceRedaction,
};
