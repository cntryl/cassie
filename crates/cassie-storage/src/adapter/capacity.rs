use std::collections::BTreeMap;

use serde::Serialize;

use crate::catalog::{IndexKind, IndexMeta};

use super::key_encoding::{self, CapacityKeyKind, CapacityKeyPrefix};
use super::{Midge, RawStorageEntry, StorageFamily, DEFAULT_FAMILY_NAME};

const FAMILY_SCHEMA: &str = "schema";
const FAMILY_DATA: &str = "data";
const FAMILY_TEMP: &str = "temp";
const FAMILY_DEFAULT: &str = "default";

const CATEGORY_ROW_BLOBS: &str = "row_blobs";
const CATEGORY_SCALAR_INDEXES: &str = "scalar_indexes";
const CATEGORY_FULLTEXT: &str = "fulltext";
const CATEGORY_VECTOR_SIDECARS: &str = "vector_sidecars";
const CATEGORY_COLUMN_BATCHES: &str = "column_batches";
const CATEGORY_PROJECTION_METADATA: &str = "projection_metadata";
const CATEGORY_TEMP_ARTIFACTS: &str = "temp_artifacts";
const CATEGORY_OTHER: &str = "other";

const CATEGORY_NAMES: &[&str] = &[
    CATEGORY_ROW_BLOBS,
    CATEGORY_SCALAR_INDEXES,
    CATEGORY_FULLTEXT,
    CATEGORY_VECTOR_SIDECARS,
    CATEGORY_COLUMN_BATCHES,
    CATEGORY_PROJECTION_METADATA,
    CATEGORY_TEMP_ARTIFACTS,
    CATEGORY_OTHER,
];

type CapacityScanEntries = Vec<RawStorageEntry>;

#[derive(Debug, Clone, Serialize)]
pub struct CapacityReport {
    pub advisory: bool,
    pub local_only: bool,
    pub persisted_metadata: bool,
    pub entries: u64,
    pub key_bytes: u64,
    pub value_bytes: u64,
    pub total_bytes: u64,
    pub families: BTreeMap<String, CapacityBucket>,
    pub categories: BTreeMap<String, CapacityBucket>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapacityBucket {
    pub supported: bool,
    pub entries: u64,
    pub key_bytes: u64,
    pub value_bytes: u64,
    pub total_bytes: u64,
}

impl CapacityBucket {
    fn supported() -> Self {
        Self {
            supported: true,
            entries: 0,
            key_bytes: 0,
            value_bytes: 0,
            total_bytes: 0,
        }
    }

    fn record(&mut self, key_bytes: u64, value_bytes: u64) {
        self.entries = self.entries.saturating_add(1);
        self.key_bytes = self.key_bytes.saturating_add(key_bytes);
        self.value_bytes = self.value_bytes.saturating_add(value_bytes);
        self.total_bytes = self.total_bytes.saturating_add(key_bytes + value_bytes);
    }
}

impl CapacityReport {
    fn empty() -> Self {
        let families = [FAMILY_SCHEMA, FAMILY_DATA, FAMILY_TEMP, FAMILY_DEFAULT]
            .into_iter()
            .map(|family| (family.to_string(), CapacityBucket::supported()))
            .collect();
        let categories = CATEGORY_NAMES
            .iter()
            .map(|category| ((*category).to_string(), CapacityBucket::supported()))
            .collect();

        Self {
            advisory: true,
            local_only: true,
            persisted_metadata: false,
            entries: 0,
            key_bytes: 0,
            value_bytes: 0,
            total_bytes: 0,
            families,
            categories,
            errors: Vec::new(),
        }
    }

    fn record(&mut self, family: &str, category: &str, key_bytes: u64, value_bytes: u64) {
        self.entries = self.entries.saturating_add(1);
        self.key_bytes = self.key_bytes.saturating_add(key_bytes);
        self.value_bytes = self.value_bytes.saturating_add(value_bytes);
        self.total_bytes = self.total_bytes.saturating_add(key_bytes + value_bytes);

        self.families
            .entry(family.to_string())
            .or_insert_with(CapacityBucket::supported)
            .record(key_bytes, value_bytes);
        self.categories
            .entry(category.to_string())
            .or_insert_with(CapacityBucket::supported)
            .record(key_bytes, value_bytes);
    }

    fn record_family_error(&mut self, family: &str, error: &impl ToString) {
        self.families
            .entry(family.to_string())
            .or_insert_with(CapacityBucket::supported)
            .supported = false;
        self.errors.push(format!("{family}: {}", error.to_string()));
    }

    fn record_family_only(&mut self, family: &str, key_bytes: u64, value_bytes: u64) {
        self.families
            .entry(family.to_string())
            .or_insert_with(CapacityBucket::supported)
            .record(key_bytes, value_bytes);
    }
}

impl Midge {
    pub fn capacity_report(&self) -> CapacityReport {
        let prefixes = key_encoding::capacity_key_prefixes();
        let mut report = CapacityReport::empty();

        for family in [
            CapacityFamily::Storage(FAMILY_SCHEMA, StorageFamily::Schema),
            CapacityFamily::Storage(FAMILY_TEMP, StorageFamily::Temp),
            CapacityFamily::Named(FAMILY_DEFAULT, DEFAULT_FAMILY_NAME),
        ] {
            let logical_name = family.logical_name();
            let entries = match family.scan(self) {
                Ok(entries) => entries,
                Err(error) => {
                    report.record_family_error(logical_name, &error);
                    continue;
                }
            };

            for (key, value) in entries {
                let category = capacity_category_for_entry(logical_name, &key, &value, &prefixes);
                report.record(
                    logical_name,
                    category,
                    usize_to_u64(key.len()),
                    usize_to_u64(value.len()),
                );
            }
        }

        let databases = match self.list_databases() {
            Ok(databases) => databases,
            Err(error) => {
                report.record_family_error(FAMILY_DATA, &error);
                return report;
            }
        };
        for database in databases {
            let entries = match self.raw_scan_prefix_database(&database.name, b"") {
                Ok(entries) => entries,
                Err(error) => {
                    report.record_family_error(&database.name, &error);
                    continue;
                }
            };
            let family_name = format!("database:{}", database.name);
            for (key, value) in entries {
                let category = capacity_category_for_entry(FAMILY_DATA, &key, &value, &prefixes);
                let key_bytes = usize_to_u64(key.len());
                let value_bytes = usize_to_u64(value.len());
                report.record(FAMILY_DATA, category, key_bytes, value_bytes);
                report.record_family_only(&family_name, key_bytes, value_bytes);
            }
        }

        report
    }
}

enum CapacityFamily<'a> {
    Storage(&'a str, StorageFamily),
    Named(&'a str, &'a str),
}

impl<'a> CapacityFamily<'a> {
    fn logical_name(&self) -> &'a str {
        match self {
            Self::Storage(name, _) | Self::Named(name, _) => name,
        }
    }

    fn scan(&self, midge: &Midge) -> Result<CapacityScanEntries, crate::adapter::CassieError> {
        match self {
            Self::Storage(_, family) => midge.raw_scan_prefix(*family, b""),
            Self::Named(_, family) => midge.raw_scan_prefix_named(family, b""),
        }
    }
}

fn capacity_category_for_entry(
    family: &str,
    key: &[u8],
    value: &[u8],
    prefixes: &[CapacityKeyPrefix],
) -> &'static str {
    if family == FAMILY_TEMP {
        return CATEGORY_TEMP_ARTIFACTS;
    }

    match capacity_key_kind(key, prefixes) {
        CapacityKeyKind::RowBlob => CATEGORY_ROW_BLOBS,
        CapacityKeyKind::ScalarIndex => CATEGORY_SCALAR_INDEXES,
        CapacityKeyKind::FullTextMetadata => CATEGORY_FULLTEXT,
        CapacityKeyKind::IndexMetadata => index_metadata_category(value),
        CapacityKeyKind::VectorSidecar => CATEGORY_VECTOR_SIDECARS,
        CapacityKeyKind::ColumnBatch => CATEGORY_COLUMN_BATCHES,
        CapacityKeyKind::ProjectionMetadata => CATEGORY_PROJECTION_METADATA,
        CapacityKeyKind::TempArtifact => CATEGORY_TEMP_ARTIFACTS,
        CapacityKeyKind::Other => CATEGORY_OTHER,
    }
}

fn capacity_key_kind(key: &[u8], prefixes: &[CapacityKeyPrefix]) -> CapacityKeyKind {
    prefixes
        .iter()
        .find(|candidate| key.starts_with(&candidate.prefix))
        .map_or(CapacityKeyKind::Other, |candidate| candidate.kind)
}

fn index_metadata_category(value: &[u8]) -> &'static str {
    let Ok(index) = serde_json::from_slice::<IndexMeta>(value) else {
        return CATEGORY_OTHER;
    };

    match index.kind {
        IndexKind::Scalar | IndexKind::TimeSeries => CATEGORY_SCALAR_INDEXES,
        IndexKind::FullText | IndexKind::Hybrid => CATEGORY_FULLTEXT,
        IndexKind::Vector => CATEGORY_VECTOR_SIDECARS,
        IndexKind::Column => CATEGORY_COLUMN_BATCHES,
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use cassie_core::types::{DataType, FieldSchema, Schema};

    use super::*;

    #[test]
    fn should_account_for_unique_reservations_as_scalar_index_bytes() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("open memory fixture");
        let collection = midge.canonical_collection_name("reserved");
        midge
            .create_collection(
                &collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "value".to_owned(),
                        data_type: DataType::Text,
                        nullable: false,
                    }],
                },
            )
            .expect("create collection");
        midge
            .put_document(
                &collection,
                Some("row".to_owned()),
                serde_json::json!({"value": "example"}),
            )
            .expect("write row");
        let documents_only = midge.capacity_report();
        let index = IndexMeta {
            definition_semantics_version: 0,
            collection: collection.clone(),
            name: "reserved_value".to_owned(),
            field: "value".to_owned(),
            fields: vec!["value".to_owned()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: true,
            options: BTreeMap::new(),
        };
        midge
            .put_index(&index)
            .expect("publish unique scalar index");
        let reservation_prefix = crate::key_codec::encode_prefix(b"unique-reservation", &[]);
        let reservations = midge
            .raw_scan_prefix_database("postgres", &reservation_prefix)
            .expect("read materialized reservation");
        let [(key, owner)] = reservations.as_slice() else {
            panic!("expected exactly one reservation");
        };
        let reservation_bytes = usize_to_u64(key.len() + owner.len());
        let before = midge.capacity_report();

        // Act
        midge
            .raw_delete(StorageFamily::Data, key)
            .expect("remove only the reservation");
        let after = midge.capacity_report();

        // Assert
        assert_eq!(owner.as_slice(), b"row");
        assert_eq!(
            before.categories[CATEGORY_SCALAR_INDEXES].total_bytes,
            after.categories[CATEGORY_SCALAR_INDEXES].total_bytes + reservation_bytes
        );
        assert_eq!(
            before.categories[CATEGORY_OTHER].total_bytes,
            after.categories[CATEGORY_OTHER].total_bytes
        );
        assert!(after.categories[CATEGORY_SCALAR_INDEXES].total_bytes > 0);
        assert!(documents_only.categories[CATEGORY_ROW_BLOBS].total_bytes > 0);
        assert_eq!(
            after.categories[CATEGORY_ROW_BLOBS].total_bytes,
            documents_only.categories[CATEGORY_ROW_BLOBS].total_bytes
        );
        assert_eq!(before.total_bytes, after.total_bytes + reservation_bytes);
        for report in [&documents_only, &before, &after] {
            assert!(report.errors.is_empty());
            assert_eq!(
                report.total_bytes,
                report
                    .categories
                    .values()
                    .map(|bucket| bucket.total_bytes)
                    .sum::<u64>()
            );
        }
    }
}
