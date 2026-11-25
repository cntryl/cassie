//! Portable operational assignment records.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalAssignmentState {
    Claimed,
    Draining,
    Released,
    Failed,
}

impl OperationalAssignmentState {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Draining => "draining",
            Self::Released => "released",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationalAssignmentMeta {
    pub assignment_id: String,
    pub node_id: String,
    pub projection_id: String,
    pub tenant: Option<String>,
    pub partition_key: Option<String>,
    pub generation: u64,
    pub state: OperationalAssignmentState,
    pub routing_hint: Option<String>,
    pub updated_ms: u64,
}
