use super::{DocumentRef, FieldCardinalityStats, FieldHeavyHitter, FieldHistogramBucket};

const MAX_HISTOGRAM_BUCKETS: usize = 8;
const MAX_HEAVY_HITTERS: usize = 8;

pub(super) fn field_cardinality_stats(
    documents: &[DocumentRef],
    field: &str,
) -> FieldCardinalityStats {
    let mut stats = FieldCardinalityStats::default();
    let mut counts = std::collections::BTreeMap::<String, u64>::new();

    for document in documents {
        match document.payload.get(field) {
            None => stats.missing_count += 1,
            Some(value) if value.is_null() => stats.null_count += 1,
            Some(value) => {
                stats.non_null_count += 1;
                let canonical = canonical_stat_value(value);
                *counts.entry(canonical.clone()).or_insert(0) += 1;
                stats.min_value = match stats.min_value.take() {
                    Some(current) => Some(current.min(canonical.clone())),
                    None => Some(canonical.clone()),
                };
                stats.max_value = match stats.max_value.take() {
                    Some(current) => Some(current.max(canonical)),
                    None => Some(canonical),
                };
            }
        }
    }

    stats.distinct_count = counts.len() as u64;
    stats.sample_count = documents.len() as u64;
    stats.histogram_buckets = histogram_buckets(&counts);
    stats.heavy_hitters = heavy_hitters(&counts);
    stats.confidence = confidence_score(&stats);
    stats
}

fn histogram_buckets(
    counts: &std::collections::BTreeMap<String, u64>,
) -> Vec<FieldHistogramBucket> {
    if counts.is_empty() {
        return Vec::new();
    }
    let entries = counts.iter().collect::<Vec<_>>();
    let bucket_count = entries.len().min(MAX_HISTOGRAM_BUCKETS);
    let mut buckets = Vec::with_capacity(bucket_count);

    for bucket_index in 0..bucket_count {
        let start = bucket_index * entries.len() / bucket_count;
        let end = ((bucket_index + 1) * entries.len() / bucket_count).max(start + 1);
        let slice = &entries[start..end.min(entries.len())];
        let Some((lower, _)) = slice.first() else {
            continue;
        };
        let Some((upper, _)) = slice.last() else {
            continue;
        };
        buckets.push(FieldHistogramBucket {
            lower: (*lower).clone(),
            upper: (*upper).clone(),
            count: slice.iter().map(|(_, count)| **count).sum(),
        });
    }

    buckets
}

fn heavy_hitters(counts: &std::collections::BTreeMap<String, u64>) -> Vec<FieldHeavyHitter> {
    let mut entries = counts
        .iter()
        .map(|(value, count)| FieldHeavyHitter {
            value: value.clone(),
            count: *count,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.value.cmp(&right.value))
    });
    entries.truncate(MAX_HEAVY_HITTERS);
    entries
}

fn confidence_score(stats: &FieldCardinalityStats) -> u8 {
    if stats.sample_count == 0 {
        return 0;
    }
    let coverage = stats
        .non_null_count
        .saturating_add(stats.null_count)
        .saturating_mul(100)
        / stats.sample_count.max(1);
    coverage.min(100) as u8
}

fn canonical_stat_value(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}
