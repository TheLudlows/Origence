//! Vendor-neutral storage query and value types.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::storage::scope::{Scope, SourceVersion};

/// A single dense embedding. `Vec<f32>` keeps the storage layer independent of
/// `pgvector::Vector`; adapters convert at the boundary.
pub type Embedding = Vec<f32>;

/// A vector query. Carries scope plus the exact profile/dimension/generation
/// that produced the vectors — never a bare vector (A2.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorQuery {
    pub scope: Scope,
    /// Optional relationally visible candidates, filtered before top-k.
    pub artifact_ids: Option<Vec<Uuid>>,
    pub profile: String,
    pub dimension: usize,
    pub generation: i64,
    pub embedding: Embedding,
    pub limit: usize,
}

/// A vector to write, tagged with the profile/generation it belongs to and
/// the source version it originates from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    pub id: Uuid,
    pub embedding: Embedding,
    pub profile: String,
    pub dimension: usize,
    pub generation: i64,
    pub source: SourceVersion,
}

/// A scored vector hit. A hit is *not yet visible*: the caller must re-verify
/// the owning source/version against the relational ledger before returning it
/// (A2.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorHit {
    pub id: Uuid,
    pub score: f32,
    pub source: SourceVersion,
}

/// Pretokenized original-chunk text for the native full-text index. The caller
/// uses the same lexical pipeline for writes and queries; adapters must not
/// stem, remove stop words or reinterpret the tokens as a query language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTextEntry {
    pub id: Uuid,
    pub source: SourceVersion,
    pub generation: i64,
    pub search_terms: String,
}

/// Native AND full-text search within one authorized scope and generation.
/// Candidate IDs are relationally eligible before native top-k. Results still
/// require a fresh relational source/version/generation and permission check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTextQuery {
    pub scope: Scope,
    pub artifact_ids: Option<Vec<Uuid>>,
    pub generation: i64,
    pub search_terms: String,
    pub limit: usize,
}

/// A native BM25 candidate, not yet authorized evidence. Cutoff ties may contain
/// any legal top-k members; callers deduplicate scoped chunk IDs and sort by
/// score then ID after final authority verification and before assigning ranks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTextHit {
    pub id: Uuid,
    pub source: SourceVersion,
    pub score: f32,
}

/// A blob key constrained by scope and a logical root; the adapter derives the
/// real path and never accepts a free-form system path (A2.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobKey {
    pub scope: Scope,
    pub root: String,
    pub path: String,
}
