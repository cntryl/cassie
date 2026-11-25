//! Portable persisted catalog declarations.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetentionPolicyMeta {
    pub name: String,
    pub collection: String,
    pub timestamp_field: String,
    pub retention_duration: String,
    pub enforcement_mode: RetentionEnforcementMode,
    pub state: RetentionPolicyState,
    pub last_enforced_ms: Option<u64>,
    pub last_deleted_rows: u64,
    pub last_skipped_rows: u64,
    pub last_error: Option<String>,
    #[serde(default)]
    pub schedule_state: RetentionScheduleState,
    #[serde(default)]
    pub next_due_at_us: Option<i64>,
    #[serde(default)]
    pub pending_cutoff_us: Option<i64>,
    #[serde(default)]
    pub pending_last_row_id: Option<String>,
    #[serde(default)]
    pub pending_scanned_rows: u64,
    #[serde(default)]
    pub pending_deleted_rows: u64,
    #[serde(default)]
    pub pending_skipped_rows: u64,
    #[serde(default)]
    pub last_successful_cutoff_us: Option<i64>,
    #[serde(default)]
    pub last_failure: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetentionEnforcementMode {
    Explicit,
}

impl RetentionEnforcementMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetentionPolicyState {
    Ready,
    Error,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetentionScheduleState {
    #[default]
    Unscheduled,
    Active,
    Suspended,
}

impl RetentionScheduleState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unscheduled => "unscheduled",
            Self::Active => "active",
            Self::Suspended => "suspended",
        }
    }
}

impl RetentionPolicyState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

impl RetentionPolicyMeta {
    #[must_use]
    pub fn new(
        name: String,
        collection: String,
        timestamp_field: String,
        retention_duration: String,
    ) -> Self {
        Self {
            name,
            collection,
            timestamp_field,
            retention_duration,
            enforcement_mode: RetentionEnforcementMode::Explicit,
            state: RetentionPolicyState::Ready,
            last_enforced_ms: None,
            last_deleted_rows: 0,
            last_skipped_rows: 0,
            last_error: None,
            schedule_state: RetentionScheduleState::Unscheduled,
            next_due_at_us: None,
            pending_cutoff_us: None,
            pending_last_row_id: None,
            pending_scanned_rows: 0,
            pending_deleted_rows: 0,
            pending_skipped_rows: 0,
            last_successful_cutoff_us: None,
            last_failure: None,
        }
    }
}
