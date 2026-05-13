//! Core primitives and API boundaries for the relay-knowledge knowledge graph.

pub mod api;
pub mod application;
pub mod code;
pub mod domain;
pub mod env;
pub mod evaluation;
pub mod indexing;
pub mod interfaces;
pub mod net;
pub mod paths;
pub mod project;
pub mod retrieval;
pub mod storage;

pub use domain::KnowledgeEntity;
pub use project::PROJECT_NAME;
