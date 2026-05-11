//! Core primitives and API boundaries for the relay-knowledge knowledge graph.

pub mod api;
pub mod application;
pub mod domain;
pub mod env;
pub mod interfaces;
pub mod net;
pub mod paths;
pub mod project;

pub use domain::KnowledgeEntity;
pub use project::PROJECT_NAME;
