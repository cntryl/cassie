//! Persisted maintenance debt metadata with SQL identifier-aware artifact keys.

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
    /// The artifact name of an `ADD COLUMN` whose backfill is still journaled.
    #[must_use]
    pub fn add_column_backfill_artifact(field: &str) -> String {
        format!(
            "add_column_backfill:{}",
            crate::column_identifier::ColumnIdentifierPath::stored_field_key(field)
        )
    }

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

#[cfg(test)]
mod tests {
    use super::MaintenanceDebtMeta;

    #[test]
    fn should_preserve_maintenance_debt_artifact_encoding() {
        // Arrange
        let metadata = MaintenanceDebtMeta::new("events", "maintenance_pending", 5, 2, None);

        // Act
        let encoded = serde_json::to_value(&metadata).expect("serialize maintenance debt");
        let artifact = MaintenanceDebtMeta::add_column_backfill_artifact("UserId");

        // Assert
        assert_eq!(encoded["target_generation"], 5);
        assert_eq!(encoded["fallback_reason"], "maintenance_pending");
        assert_eq!(artifact, r#"add_column_backfill:"UserId""#);
    }
}
