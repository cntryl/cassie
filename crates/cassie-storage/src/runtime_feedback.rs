use std::io::{self, Write};

use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "runtime_feedback_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeFeedbackKey {
    pub schema_epoch: u64,
    pub database: Option<String>,
    #[serde(default)]
    pub search_path: Vec<String>,
    pub collection: String,
    pub operator_family: String,
    pub relation_set: Vec<String>,
    pub predicate_shape_hash: u64,
    pub index_shape_hash: Option<u64>,
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
    pub stable_samples: u64,
    pub outlier_samples: u64,
    pub confidence_bps: u16,
    pub stable_rows_in_total: u64,
    pub stable_rows_out_total: u64,
    pub stable_elapsed_ms_total: u64,
    pub stable_storage_reads_total: u64,
    pub stable_storage_writes_total: u64,
    pub stable_temp_writes_total: u64,
    pub stable_candidate_count_total: u64,
    pub stable_result_count_total: u64,
    pub spill_samples: u64,
    pub memory_pressure_samples: u64,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

impl RuntimeFeedbackRecord {
    #[must_use]
    pub fn stable_average_rows_in(&self) -> u64 {
        average(self.stable_rows_in_total, self.stable_samples)
    }

    #[must_use]
    pub fn stable_average_rows_out(&self) -> u64 {
        average(self.stable_rows_out_total, self.stable_samples)
    }

    #[must_use]
    pub fn stable_average_elapsed_ms(&self) -> u64 {
        average(self.stable_elapsed_ms_total, self.stable_samples)
    }

    #[must_use]
    pub fn stable_average_storage_reads(&self) -> u64 {
        average(self.stable_storage_reads_total, self.stable_samples)
    }

    #[must_use]
    pub fn stable_average_storage_writes(&self) -> u64 {
        average(self.stable_storage_writes_total, self.stable_samples)
    }

    #[must_use]
    pub fn stable_average_temp_writes(&self) -> u64 {
        average(self.stable_temp_writes_total, self.stable_samples)
    }

    #[must_use]
    pub fn stable_average_candidate_count(&self) -> u64 {
        average(self.stable_candidate_count_total, self.stable_samples)
    }

    #[must_use]
    pub fn stable_average_result_count(&self) -> u64 {
        average(self.stable_result_count_total, self.stable_samples)
    }
}

#[derive(Debug, Default)]
pub struct RuntimeFeedbackMutation {
    pub upserts: Vec<(RuntimeFeedbackKey, RuntimeFeedbackRecord)>,
    pub deletes: Vec<RuntimeFeedbackKey>,
}

impl RuntimeFeedbackMutation {
    pub fn upsert(&mut self, key: RuntimeFeedbackKey, record: RuntimeFeedbackRecord) {
        self.deletes.retain(|deleted| deleted != &key);
        if let Some((_, existing)) = self
            .upserts
            .iter_mut()
            .find(|(existing_key, _)| existing_key == &key)
        {
            *existing = record;
        } else {
            self.upserts.push((key, record));
        }
    }

    pub fn delete(&mut self, key: RuntimeFeedbackKey) {
        self.upserts
            .retain(|(existing_key, _)| existing_key != &key);
        if !self.deletes.contains(&key) {
            self.deletes.push(key);
        }
    }

    pub fn merge(&mut self, other: Self) {
        for key in other.deletes {
            self.delete(key);
        }
        for (key, record) in other.upserts {
            self.upsert(key, record);
        }
    }
}

fn average(total: u64, samples: u64) -> u64 {
    if samples == 0 {
        0
    } else {
        total
            .saturating_add(samples.saturating_sub(1))
            .checked_div(samples)
            .unwrap_or(0)
    }
}

/// Hashes the existing JSON serialization used in durable feedback keys.
///
/// # Panics
///
/// Panics when the value cannot be serialized as JSON.
#[must_use]
pub fn stable_fingerprint<T: Serialize>(value: &T) -> u64 {
    let mut writer = StableFingerprintWriter::default();
    serde_json::to_writer(&mut writer, value).expect("serialize stable fingerprint");
    writer.finish()
}

#[derive(Default)]
struct StableFingerprintWriter {
    state: u64,
}

impl StableFingerprintWriter {
    fn finish(&self) -> u64 {
        self.state
    }
}

impl Write for StableFingerprintWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0100_0000_01b3;

        if self.state == 0 {
            self.state = FNV_OFFSET_BASIS;
        }
        for byte in buf {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
