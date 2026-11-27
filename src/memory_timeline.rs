//! Shared wire types for dated memory conclusions and their original evidence.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::memory_semantics::Statement;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source_event_id: Uuid,
    pub statement: Statement,
    pub effective_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConclusionState {
    Unknown,
    Conflicting,
    Current { value: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPeriod {
    pub effective_from_ms: Option<i64>,
    pub state: ConclusionState,
}
