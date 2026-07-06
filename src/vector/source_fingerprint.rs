use crate::embeddings::NormalizedVectorRecord;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

#[must_use]
pub fn normalized_vector_source_fingerprint(records: &[NormalizedVectorRecord]) -> u64 {
    let mut sorted = records.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));

    let mut hasher = StableHasher::new();
    hasher.write_usize(sorted.len());
    for record in sorted {
        hasher.write_str(&record.id);
        hasher.write_str(record.metric.as_str());
        hasher.write_usize(record.dimensions);
        hasher.write_u32(record.normalization_version);
        hasher.write_u8(u8::from(record.payload_available));
        hasher.write_u64(record.magnitude.to_bits());
        hasher.write_usize(record.values.len());
        for value in &record.values {
            hasher.write_u32(value.to_bits());
        }
    }
    hasher.finish()
}

struct StableHasher {
    state: u64,
}

impl StableHasher {
    fn new() -> Self {
        Self { state: FNV_OFFSET }
    }

    fn finish(self) -> u64 {
        self.state
    }

    fn write_u8(&mut self, value: u8) {
        self.state ^= u64::from(value);
        self.state = self.state.wrapping_mul(FNV_PRIME);
    }

    fn write_u32(&mut self, value: u32) {
        self.write_bytes(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(u64::try_from(value).unwrap_or(u64::MAX));
    }

    fn write_str(&mut self, value: &str) {
        self.write_usize(value.len());
        self.write_bytes(value.as_bytes());
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u8(*byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalized_vector_source_fingerprint;
    use crate::embeddings::{DistanceMetric, NormalizedVectorRecord};

    fn record(id: &str, values: Vec<f32>) -> NormalizedVectorRecord {
        NormalizedVectorRecord {
            collection: "docs".to_string(),
            field: "embedding".to_string(),
            id: id.to_string(),
            dimensions: values.len(),
            metric: DistanceMetric::Cosine,
            normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
            payload_available: true,
            magnitude: 1.0,
            values,
        }
    }

    #[test]
    fn should_fingerprint_normalized_records_independent_of_input_order() {
        // Arrange
        let first = vec![record("a", vec![1.0, 0.0]), record("b", vec![0.0, 1.0])];
        let second = vec![record("b", vec![0.0, 1.0]), record("a", vec![1.0, 0.0])];

        // Act
        let first_fingerprint = normalized_vector_source_fingerprint(&first);
        let second_fingerprint = normalized_vector_source_fingerprint(&second);

        // Assert
        assert_eq!(first_fingerprint, second_fingerprint);
    }

    #[test]
    fn should_change_fingerprint_when_vector_bits_change() {
        // Arrange
        let original = vec![record("a", vec![1.0, 0.0])];
        let changed = vec![record("a", vec![1.0, f32::from_bits(1)])];

        // Act
        let original_fingerprint = normalized_vector_source_fingerprint(&original);
        let changed_fingerprint = normalized_vector_source_fingerprint(&changed);

        // Assert
        assert_ne!(original_fingerprint, changed_fingerprint);
    }
}
