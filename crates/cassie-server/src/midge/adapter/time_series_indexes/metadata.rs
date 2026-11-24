use super::CassieError;

pub(super) const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct TimeSeriesManifest {
    pub(super) version: u32,
    pub(super) generation: u64,
    pub(super) total_membership: u64,
}

impl TimeSeriesManifest {
    pub(super) const fn empty(generation: u64) -> Self {
        Self {
            version: FORMAT_VERSION,
            generation,
            total_membership: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct BucketIdentity {
    pub(super) partition: String,
    pub(super) start_seconds: i64,
}

pub(super) fn encode_manifest(manifest: &TimeSeriesManifest) -> Result<Vec<u8>, CassieError> {
    serde_json::to_vec(manifest)
        .map_err(|error| CassieError::Execution(format!("serialize time-series manifest: {error}")))
}

pub(super) fn decode_manifest(raw: &[u8]) -> Result<TimeSeriesManifest, CassieError> {
    serde_json::from_slice(raw)
        .map_err(|error| CassieError::Parse(format!("invalid time-series manifest: {error}")))
}

pub(super) const fn encode_count(count: u64) -> [u8; 8] {
    count.to_be_bytes()
}

pub(super) fn decode_count(raw: &[u8]) -> Result<u64, CassieError> {
    let bytes: [u8; 8] = raw
        .try_into()
        .map_err(|_| CassieError::Parse("invalid time-series bucket count".to_string()))?;
    Ok(u64::from_be_bytes(bytes))
}
