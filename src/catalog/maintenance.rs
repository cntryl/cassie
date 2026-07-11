use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaintenanceDebtMeta {
    pub collection: String,
    pub artifact: String,
    pub target_generation: u64,
    pub retry_count: u32,
    pub last_error: Option<String>,
    pub fallback_reason: String,
}

impl MaintenanceDebtMeta {
    #[must_use]
    pub fn new(
        collection: impl Into<String>,
        artifact: impl Into<String>,
        target_generation: u64,
        retry_count: u32,
        last_error: Option<String>,
    ) -> Self {
        Self {
            collection: collection.into(),
            artifact: artifact.into(),
            target_generation,
            retry_count,
            last_error,
            fallback_reason: "maintenance_pending".to_string(),
        }
    }
}
