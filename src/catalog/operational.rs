use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalAssignmentState {
    Claimed,
    Draining,
    Released,
    Failed,
}

impl OperationalAssignmentState {
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

impl Catalog {
    pub fn register_operational_assignment(&self, metadata: OperationalAssignmentMeta) {
        self.operational_assignments
            .write()
            .insert(metadata.assignment_id.clone(), metadata);
        self.bump_version();
    }

    pub fn list_operational_assignments(&self) -> Vec<OperationalAssignmentMeta> {
        let mut out = self
            .operational_assignments
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by(|left, right| {
            left.projection_id
                .cmp(&right.projection_id)
                .then_with(|| left.tenant.cmp(&right.tenant))
                .then_with(|| left.partition_key.cmp(&right.partition_key))
                .then_with(|| left.assignment_id.cmp(&right.assignment_id))
        });
        out
    }
}
