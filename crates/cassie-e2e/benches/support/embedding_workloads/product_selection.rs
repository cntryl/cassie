//! Exact selectors for the hosted vector product metric shards.

use std::ops::Range;

use sha2::{Digest, Sha256};

use super::vector_file::hex;

pub const QUERY_SHARD_SIZE: usize = 250;
pub const QUERY_SPLIT_SIZE: usize = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Configuration {
    Exact,
    Hnsw(usize),
    Ivfflat(usize),
}

/// Parses the finite product configuration set.
///
/// # Errors
///
/// Returns an error for a selector outside the committed five-configuration matrix.
pub fn configuration(value: &str) -> Result<Configuration, String> {
    match value {
        "exact" => Ok(Configuration::Exact),
        "hnsw-1" => Ok(Configuration::Hnsw(0)),
        "hnsw-2" => Ok(Configuration::Hnsw(1)),
        "ivfflat-1" => Ok(Configuration::Ivfflat(0)),
        "ivfflat-2" => Ok(Configuration::Ivfflat(1)),
        _ => Err(format!(
            "unsupported product vector configuration {value:?}"
        )),
    }
}

/// Parses one aligned quarter of each 1,000-ID query split.
///
/// # Errors
///
/// Returns an error unless `value` is one of the four exact 250-ID ranges.
pub fn query_range(value: &str) -> Result<Range<usize>, String> {
    let (start, end) = value
        .split_once("..")
        .ok_or_else(|| format!("invalid product vector query range {value:?}"))?;
    let start = start
        .parse::<usize>()
        .map_err(|error| format!("invalid product vector range start: {error}"))?;
    let end = end
        .parse::<usize>()
        .map_err(|error| format!("invalid product vector range end: {error}"))?;
    if end.saturating_sub(start) != QUERY_SHARD_SIZE
        || start % QUERY_SHARD_SIZE != 0
        || end > QUERY_SPLIT_SIZE
    {
        return Err(format!(
            "product vector query range must be an aligned 250-ID shard within 0..{QUERY_SPLIT_SIZE}"
        ));
    }
    Ok(start..end)
}

/// Verifies a selected shard against the split size declared by the corpus manifest.
///
/// # Errors
///
/// Returns an error when a shard would leave the actual query split.
pub fn validate_split_range(range: &Range<usize>, split_size: usize) -> Result<(), String> {
    if range.end > split_size {
        return Err(format!(
            "product vector range {}..{} exceeds split size {split_size}",
            range.start, range.end
        ));
    }
    Ok(())
}

/// Converts a split-local shard into query-vector ordinals.
///
/// # Errors
///
/// Returns an error when the shard extends beyond its split.
pub fn absolute_query_range(
    split_start: usize,
    split_size: usize,
    shard: &Range<usize>,
) -> Result<Range<usize>, String> {
    validate_split_range(shard, split_size)?;
    Ok((split_start + shard.start)..(split_start + shard.end))
}

/// Stable cache key for a particular exact-query subset, not only its length.
#[must_use]
pub fn truth_cache_key(queries: &[usize]) -> String {
    let mut hasher = Sha256::new();
    for query in queries {
        hasher.update(u64::try_from(*query).unwrap_or(u64::MAX).to_le_bytes());
    }
    hex(&hasher.finalize())
}
