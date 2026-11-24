//! Borrow metadata only while formatting the selected EXPLAIN output.

use std::cmp::Ordering;

use crate::catalog::{name_matches, IndexMeta, ProjectionKind, ProjectionMeta};

use super::Catalog;

impl Catalog {
    pub(crate) fn with_index_metadata<R>(
        &self,
        collection: &str,
        name: &str,
        use_metadata: impl FnOnce(Option<&IndexMeta>) -> R,
    ) -> R {
        let indexes = self.indexes.read();
        // Same exact composite key preference as get_index, without a temporary key String.
        let metadata = indexes
            .iter()
            .find(|(key, _)| {
                key.strip_prefix(collection)
                    .and_then(|suffix| suffix.strip_prefix(':'))
                    == Some(name)
            })
            .map(|(_, metadata)| metadata)
            .or_else(|| {
                indexes.values().find(|metadata| {
                    name_matches(&metadata.collection, collection)
                        && name_matches(&metadata.name, name)
                })
            });
        use_metadata(metadata)
    }

    pub(crate) fn with_analytical_projection_name<R>(
        &self,
        source: &str,
        use_name: impl FnOnce(Option<&str>) -> R,
    ) -> R {
        let projections = self.projections.read();
        // Stable sorting in list_projection_metadata chooses the first original value on a tie.
        let selected = projections
            .values()
            .filter(|projection| {
                projection
                    .materialized
                    .as_ref()
                    .is_some_and(|materialized| {
                        materialized
                            .options
                            .get("analytical")
                            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                            && materialized
                                .source_collections
                                .iter()
                                .any(|candidate| candidate == source)
                    })
            })
            .fold(
                None::<&ProjectionMeta>,
                |selected, candidate| match selected {
                    Some(previous)
                        if ascii_lowercase_cmp(&previous.collection, &candidate.collection)
                            != Ordering::Greater =>
                    {
                        Some(previous)
                    }
                    _ => Some(candidate),
                },
            );
        use_name(selected.map(|projection| projection.collection.as_str()))
    }

    pub(crate) fn materialized_projection_freshness(&self, collection: &str) -> &'static str {
        let projections = self.projections.read();
        let named = projections
            .get(collection)
            .or_else(|| {
                projections
                    .iter()
                    .find(|(stored, metadata)| {
                        metadata.kind == ProjectionKind::Materialized
                            && name_matches(stored, collection)
                    })
                    .map(|(_, metadata)| metadata)
            })
            .filter(|metadata| metadata.kind == ProjectionKind::Materialized);
        named
            .or_else(|| {
                projections.values().find(|metadata| {
                    metadata.kind == ProjectionKind::Materialized
                        && metadata
                            .versions
                            .iter()
                            .any(|version| version.output_collection == collection)
                })
            })
            .map_or("unavailable", |metadata| metadata.freshness.as_str())
    }
}

fn ascii_lowercase_cmp(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}
