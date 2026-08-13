//! Bounded source parsing, artifact storage, and policy-candidate extraction adapters.

mod artifact_store;
mod chunking;
mod extraction;
mod openrouter;
pub mod parser;

pub use artifact_store::{MemoryArtifactStore, OpenDalArtifactStore};
pub use chunking::{PolicyChunk, chunk_document};
pub use extraction::{HeuristicPolicyExtractionModel, validate_extraction_response};
pub use openrouter::{ConfiguredPolicyExtractionModel, OpenRouterPolicyExtractionModel};
pub use parser::SafePolicyDocumentParser;
