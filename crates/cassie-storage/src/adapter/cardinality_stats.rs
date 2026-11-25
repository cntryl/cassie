use cassie_core::catalog::storage_metadata::compare_histogram_endpoints;

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
                *counts.entry(canonical).or_insert(0) += 1;
            }
        }
    }

    stats.distinct_count = counts.len() as u64;
    stats.sample_count = documents.len() as u64;
    let mut entries = counts.iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| compare_histogram_endpoints(left, right));
    stats.min_value = entries.first().map(|(value, _)| (*value).clone());
    stats.max_value = entries.last().map(|(value, _)| (*value).clone());
    stats.histogram_buckets = histogram_buckets(&entries);
    stats.heavy_hitters = heavy_hitters(&counts);
    stats.confidence = confidence_score(&stats);
    stats
}

fn histogram_buckets(entries: &[(&String, &u64)]) -> Vec<FieldHistogramBucket> {
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

#[cfg(test)]
mod tests {
    use cassie_core::types::{DataType, FieldSchema, Schema};

    use crate::adapter::Midge;

    #[test]
    fn should_order_rebuilt_integer_histograms_by_exact_value() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("open memory fixture");
        type Fixture<'a> = (&'a str, DataType, &'a [i64], &'a [(&'a str, u64)]);
        let samples: [Fixture<'_>; 2] = [
            ("numbers", DataType::Int, &[2, 10], &[("2", 1), ("10", 1)]),
            (
                "wide_numbers",
                DataType::BigInt,
                &[
                    9_007_199_254_740_993,
                    -2,
                    10,
                    -10,
                    9_007_199_254_740_992,
                    10,
                ],
                &[
                    ("-10", 1),
                    ("-2", 1),
                    ("10", 2),
                    ("9007199254740992", 1),
                    ("9007199254740993", 1),
                ],
            ),
        ];
        for (collection, data_type, values, expected) in samples {
            midge
                .create_collection(
                    collection,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "value".to_owned(),
                            data_type,
                            nullable: false,
                        }],
                    },
                )
                .expect("create typed collection");
            for (offset, value) in values.iter().enumerate() {
                midge
                    .put_document(
                        collection,
                        Some(offset.to_string()),
                        serde_json::json!({"value": value}),
                    )
                    .expect("write integer row");
            }

            // Act
            let stats = midge
                .rebuild_cardinality_stats_for_collection(collection)
                .expect("rebuild persisted field stats");

            // Assert
            let field = &stats.fields["value"];
            assert_eq!(
                field.min_value.as_deref(),
                expected.first().map(|item| item.0)
            );
            assert_eq!(
                field.max_value.as_deref(),
                expected.last().map(|item| item.0)
            );
            assert_eq!(field.distinct_count, u64::try_from(expected.len()).unwrap());
            assert_eq!(field.histogram_buckets.len(), expected.len());
            for (bucket, (value, count)) in field.histogram_buckets.iter().zip(expected) {
                assert_eq!(
                    (bucket.lower.as_str(), bucket.upper.as_str(), bucket.count),
                    (*value, *value, *count)
                );
            }
        }
    }
}
