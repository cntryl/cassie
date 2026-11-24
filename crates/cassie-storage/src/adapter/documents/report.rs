use serde_json::{Map, Value};

const MAX_ROLLUP_CHANGE_REPORT_BYTES: usize = 64 * 1024;
const PROJECTED_ENTRY_BYTES: usize = std::mem::size_of::<(String, Value)>();

#[derive(Debug, Clone, Default)]
pub struct RollupSourceChangeReport {
    pub rows: Vec<RollupSourceRowChange>,
    pub exceeded_limit: bool,
    retained_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct RollupSourceRowChange {
    pub before: Option<Map<String, Value>>,
    pub after: Option<Map<String, Value>>,
}

impl RollupSourceChangeReport {
    pub fn record(&mut self, before: Option<&Value>, after: Option<&Value>, fields: &[String]) {
        if self.exceeded_limit {
            return;
        }
        let Some(before_bytes) =
            projected_payload_size_within(before, fields, MAX_ROLLUP_CHANGE_REPORT_BYTES)
        else {
            self.exceed_limit();
            return;
        };
        let Some(after_bytes) =
            projected_payload_size_within(after, fields, MAX_ROLLUP_CHANGE_REPORT_BYTES)
        else {
            self.exceed_limit();
            return;
        };
        let Some(row_bytes) = before_bytes.checked_add(after_bytes) else {
            self.exceed_limit();
            return;
        };
        let previous_capacity_bytes = self
            .rows
            .capacity()
            .checked_mul(std::mem::size_of::<RollupSourceRowChange>());
        let Some(previous_capacity_bytes) = previous_capacity_bytes else {
            self.exceed_limit();
            return;
        };
        let Some(existing_content_bytes) = self.retained_bytes.checked_sub(previous_capacity_bytes)
        else {
            self.exceed_limit();
            return;
        };
        if self.rows.try_reserve_exact(1).is_err() {
            self.exceed_limit();
            return;
        }
        let Some(capacity_bytes) = self
            .rows
            .capacity()
            .checked_mul(std::mem::size_of::<RollupSourceRowChange>())
        else {
            self.exceed_limit();
            return;
        };
        let Some(total_bytes) = existing_content_bytes
            .checked_add(row_bytes)
            .and_then(|bytes| bytes.checked_add(capacity_bytes))
        else {
            self.exceed_limit();
            return;
        };
        if total_bytes > MAX_ROLLUP_CHANGE_REPORT_BYTES {
            self.exceed_limit();
            return;
        }
        let before = before.map(|payload| clone_projected_payload(payload, fields));
        let after = after.map(|payload| clone_projected_payload(payload, fields));
        let row = RollupSourceRowChange { before, after };
        self.rows.push(row);
        self.retained_bytes = total_bytes;
    }

    fn exceed_limit(&mut self) {
        self.rows = Vec::new();
        self.retained_bytes = 0;
        self.exceeded_limit = true;
    }
}

fn projected_payload_size_within(
    payload: Option<&Value>,
    fields: &[String],
    maximum_bytes: usize,
) -> Option<usize> {
    let bytes = projected_payload_size(payload, fields)?;
    (bytes <= maximum_bytes).then_some(bytes)
}

fn projected_payload_size(payload: Option<&Value>, fields: &[String]) -> Option<usize> {
    let Some(payload) = payload else {
        return Some(0);
    };
    let source = payload.as_object()?;
    // The cap bounds logical report content: one map entry per selected field
    // plus its owned key and scalar bytes. Allocator slack inside the map's
    // nodes is deliberately excluded so admission does not depend on packing.
    let mut bytes = 0usize;
    for field in fields {
        let Some(value) = source.get(field) else {
            continue;
        };
        bytes = bytes
            .checked_add(PROJECTED_ENTRY_BYTES)?
            .checked_add(field.len())?
            .checked_add(scalar_value_bytes(value)?)?;
    }
    Some(bytes)
}

/// Copies the selected fields. The size preflight has already rejected
/// payloads that are not objects, so a non-object projects to an empty map.
fn clone_projected_payload(payload: &Value, fields: &[String]) -> Map<String, Value> {
    let mut projected = Map::new();
    if let Some(source) = payload.as_object() {
        for field in fields {
            if let Some(value) = source.get(field) {
                projected.insert(field.clone(), value.clone());
            }
        }
    }
    projected
}

fn scalar_value_bytes(value: &Value) -> Option<usize> {
    match value {
        Value::Null | Value::Bool(_) => Some(std::mem::size_of::<Value>()),
        Value::Number(number) => Some(number.to_string().len()),
        Value::String(value) => Some(value.len()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

#[derive(Debug, Default)]
pub struct DocumentWriteBatchReport {
    pub relation_id: u64,
    pub ids: Vec<String>,
    pub changed_ids: Vec<String>,
    pub row_delta: i64,
    pub stats: crate::operation_stats::ProjectionWriteStats,
    pub data_epoch: Option<u64>,
    pub rollup_changes: RollupSourceChangeReport,
}

impl DocumentWriteBatchReport {
    pub(super) fn has_changes(&self) -> bool {
        self.stats.row_puts > 0
            || self.stats.row_deletes > 0
            || self.stats.index_puts > 0
            || self.stats.index_deletes > 0
            || self.stats.metadata_puts > 0
            || self.stats.metadata_deletes > 0
    }
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
