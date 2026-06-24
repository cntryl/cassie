use std::collections::BTreeMap;

use serde::Serialize;

use crate::catalog::{IndexKind, IndexMeta};

use super::key_encoding::{self, CapacityKeyKind, CapacityKeyPrefix};
use super::{Midge, StorageFamily, DEFAULT_FAMILY_NAME};

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

    fn record_family_error(&mut self, family: &str, error: impl ToString) {
        self.families
            .entry(family.to_string())
            .or_insert_with(CapacityBucket::supported)
            .supported = false;
        self.errors.push(format!("{family}: {}", error.to_string()));
    }
}

impl Midge {
    pub(crate) fn capacity_report(&self) -> CapacityReport {
        let prefixes = key_encoding::capacity_key_prefixes();
        let mut report = CapacityReport::empty();

        for family in [
            CapacityFamily::Storage(FAMILY_SCHEMA, StorageFamily::Schema),
            CapacityFamily::Storage(FAMILY_DATA, StorageFamily::Data),
            CapacityFamily::Storage(FAMILY_TEMP, StorageFamily::Temp),
            CapacityFamily::Named(FAMILY_DEFAULT, DEFAULT_FAMILY_NAME),
        ] {
            let logical_name = family.logical_name();
            let entries = match family.scan(self) {
                Ok(entries) => entries,
                Err(error) => {
                    report.record_family_error(logical_name, error);
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

    fn scan(&self, midge: &Midge) -> Result<Vec<(Vec<u8>, Vec<u8>)>, crate::app::CassieError> {
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
        .map(|candidate| candidate.kind)
        .unwrap_or(CapacityKeyKind::Other)
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
