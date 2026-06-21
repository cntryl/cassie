use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeFeedbackKey {
    pub sql_fingerprint: u64,
    pub schema_epoch: u64,
    pub database: Option<String>,
    pub collection: String,
    pub operator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeFeedbackRecord {
    pub executions: u64,
    pub rows_in_total: u64,
    pub rows_out_total: u64,
    pub elapsed_ms_total: u64,
    pub storage_reads_total: u64,
    pub storage_writes_total: u64,
    pub temp_writes_total: u64,
    pub candidate_count_total: u64,
    pub result_count_total: u64,
    pub errors_total: u64,
    pub last_error_class: Option<String>,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeFeedbackObservation {
    pub rows_in: u64,
    pub rows_out: u64,
    pub elapsed_ms: u64,
    pub storage_reads: u64,
    pub storage_writes: u64,
    pub temp_writes: u64,
    pub candidate_count: u64,
    pub result_count: u64,
    pub error_class: Option<String>,
}
