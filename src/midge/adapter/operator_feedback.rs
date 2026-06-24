use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRuntimeFeedbackRecord {
    key: crate::runtime::RuntimeFeedbackKey,
    record: crate::runtime::RuntimeFeedbackRecord,
}

impl Midge {
    pub fn list_runtime_feedback_records(
        &self,
    ) -> Result<
        Vec<(
            crate::runtime::RuntimeFeedbackKey,
            crate::runtime::RuntimeFeedbackRecord,
        )>,
        CassieError,
    > {
        let entries =
            self.raw_scan_prefix(StorageFamily::Schema, &Self::runtime_feedback_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice::<StoredRuntimeFeedbackRecord>(&raw_value)
            else {
                continue;
            };
            out.push((record.key, record.record));
        }
        out.sort_by_key(|entry| entry.1.last_seen_ms);
        Ok(out)
    }

    pub fn replace_runtime_feedback_records(
        &self,
        records: &[(
            crate::runtime::RuntimeFeedbackKey,
            crate::runtime::RuntimeFeedbackRecord,
        )],
    ) -> Result<(), CassieError> {
        let existing =
            self.raw_scan_prefix(StorageFamily::Schema, &Self::runtime_feedback_prefix())?;
        let mut tx = self.begin_schema_rw_tx()?;
        for (key, _value) in existing {
            tx.delete(key).map_err(CassieError::from)?;
        }
        for (key, record) in records {
            let stored = StoredRuntimeFeedbackRecord {
                key: key.clone(),
                record: record.clone(),
            };
            let value = serde_json::to_vec(&stored)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(Self::runtime_feedback_key(key), value, None)
                .map_err(CassieError::from)?;
        }
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn clear_runtime_feedback_records(&self) -> Result<(), CassieError> {
        self.replace_runtime_feedback_records(&[])
    }
}
