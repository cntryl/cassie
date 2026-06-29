use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use super::{Cassie, CassieError, current_time_millis};
use crate::catalog::{
    ProjectionConsistencyReportMeta, ProjectionManifestHashMetadata,
    ProjectionManifestRangeSummary, ProjectionManifestRootSummary,
    ProjectionManifestRowHashSummary, ProjectionVerificationManifest,
};

const PROJECTION_MANIFEST_VERSION: u16 = 1;
const DEFAULT_MANIFEST_TTL_MS: u64 = 86_400_000;

#[derive(Debug, Clone)]
pub struct ProjectionManifestExportOptions {
    pub instance_id: String,
    pub generated_ms: Option<u64>,
    pub ttl_ms: Option<u64>,
    pub include_row_hashes: bool,
}

impl ProjectionManifestExportOptions {
    pub fn for_instance(instance_id: impl Into<String>) -> Self {
        Self {
            instance_id: instance_id.into(),
            generated_ms: None,
            ttl_ms: None,
            include_row_hashes: false,
        }
    }
}

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn export_projection_verification_manifest(
        &self,
        projection: &str,
        options: ProjectionManifestExportOptions,
    ) -> Result<ProjectionVerificationManifest, CassieError> {
        let metadata = self
            .catalog
            .get_projection_metadata(projection)
            .or_else(|| self.midge.projection_metadata(projection).ok().flatten())
            .ok_or_else(|| CassieError::NotFound(format!("projection not found: {projection}")))?;
        let hash_collection = metadata.active_output_collection().unwrap_or(projection);
        let root = self.midge.root_hash(hash_collection)?;
        let mut ranges = self.midge.list_range_hashes(hash_collection)?;
        ranges.sort_by_key(|range| range.range_id);
        let generated_ms = options.generated_ms.unwrap_or_else(current_time_millis);
        let ttl_ms = options.ttl_ms.unwrap_or(DEFAULT_MANIFEST_TTL_MS);
        let schema_epoch = root
            .as_ref()
            .map_or(u64::from(metadata.schema_version), |root| root.schema_epoch);
        let projection_definition_hash = metadata
            .materialized
            .as_ref()
            .map(|materialized| materialized.definition_fingerprint)
            .or_else(|| {
                metadata
                    .active_version
                    .as_ref()
                    .and_then(|version_id| {
                        metadata
                            .versions
                            .iter()
                            .find(|version| &version.version_id == version_id)
                    })
                    .map(|version| version.definition_fingerprint)
            });
        let mut manifest = ProjectionVerificationManifest {
            manifest_version: PROJECTION_MANIFEST_VERSION,
            instance_id: options.instance_id,
            projection_id: metadata.projection_id().to_string(),
            projection_version_id: metadata.active_version.clone().or_else(|| {
                root.as_ref()
                    .and_then(|root| root.version_id.as_ref().map(ToString::to_string))
            }),
            projection_kind: metadata.kind.as_str().to_string(),
            schema_epoch,
            projection_definition_hash,
            source_identity: metadata.source_identity.clone(),
            source_checkpoint: metadata.source_checkpoint.clone(),
            source_position: metadata.source_position,
            generated_ms,
            expires_at_ms: generated_ms.saturating_add(ttl_ms),
            hash: root.as_ref().map_or_else(|| {
                ProjectionManifestHashMetadata {
                    algorithm: metadata.hashes.algorithm.algorithm.clone(),
                    digest_length: metadata.hashes.algorithm.digest_length,
                    canonical_encoder_version: metadata.hashes.algorithm.canonical_encoder_version,
                    row_hash_version: metadata.hashes.algorithm.hash_version,
                    range_hash_version: metadata.hashes.algorithm.hash_version,
                    root_hash_version: metadata.hashes.algorithm.hash_version,
                }
            }, root_hash_metadata),
            root: root.as_ref().map(root_summary),
            ranges: ranges.iter().map(range_summary).collect(),
            row_hashes: if options.include_row_hashes {
                row_hash_summaries(&self.midge, hash_collection)?
            } else {
                Vec::new()
            },
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest_digest(&manifest);
        self.runtime
            .record_projection_manifest_export(metadata.projection_id().to_string());
        Ok(manifest)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn compare_projection_verification_manifests(
        &self,
        manifests: Vec<ProjectionVerificationManifest>,
    ) -> Result<ProjectionConsistencyReportMeta, CassieError> {
        let mut manifests = manifests;
        manifests.sort_by(|left, right| {
            (
                &left.projection_id,
                &left.projection_version_id,
                &left.instance_id,
            )
                .cmp(&(
                    &right.projection_id,
                    &right.projection_version_id,
                    &right.instance_id,
                ))
        });
        let created_ms = current_time_millis();
        let report = build_consistency_report(manifests, created_ms);
        self.midge
            .put_projection_consistency_report(report.clone())?;
        self.catalog
            .register_projection_consistency_report(report.clone());
        self.runtime.record_projection_consistency_check(
            report.projection_id.clone(),
            report.state.clone(),
            report.mismatch_count,
            report.stale_manifest_count,
            report.incompatible_manifest_count,
        );
        Ok(report)
    }
}

fn build_consistency_report(
    manifests: Vec<ProjectionVerificationManifest>,
    created_ms: u64,
) -> ProjectionConsistencyReportMeta {
    let mut diagnostics = BTreeSet::new();
    if manifests.len() < 2 {
        diagnostics.insert("requires-two-manifests".to_string());
    }
    let instance_ids = manifests
        .iter()
        .map(|manifest| manifest.instance_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let base = manifests.first();
    let mut incompatible_manifest_count = 0;
    if let Some(base) = base {
        for manifest in manifests.iter().skip(1) {
            let before = diagnostics.len();
            record_incompatibilities(base, manifest, &mut diagnostics);
            if diagnostics.len() > before {
                incompatible_manifest_count += 1;
            }
        }
    }

    let stale_manifest_count = manifests
        .iter()
        .filter(|manifest| manifest_is_stale(manifest, created_ms))
        .count() as u64;
    let unverifiable_count = manifests
        .iter()
        .filter(|manifest| manifest_is_unverifiable(manifest))
        .count() as u64;
    let (divergent_range_count, divergent_row_count) =
        divergence_counts(&manifests, &mut diagnostics);
    let root_mismatch = manifests
        .first()
        .and_then(|manifest| manifest.root.as_ref())
        .is_some_and(|root| {
            manifests
                .iter()
                .filter_map(|manifest| manifest.root.as_ref())
                .any(|other| other.digest != root.digest)
        });
    let state = if manifests.len() < 2 || unverifiable_count > 0 {
        "unverifiable"
    } else if incompatible_manifest_count > 0 {
        "incompatible"
    } else if stale_manifest_count > 0 {
        "stale"
    } else if root_mismatch {
        "divergent"
    } else {
        "consistent"
    };
    let mismatch_count = if state == "divergent" {
        divergent_row_count.max(divergent_range_count).max(1)
    } else {
        0
    };
    let compatibility_status = match state {
        "consistent" | "divergent" => "compatible",
        other => other,
    }
    .to_string();
    ProjectionConsistencyReportMeta {
        report_id: format!("projection-consistency-{}", uuid::Uuid::new_v4()),
        created_ms,
        projection_id: base
            .map(|manifest| manifest.projection_id.clone())
            .unwrap_or_default(),
        projection_version_id: base.and_then(|manifest| manifest.projection_version_id.clone()),
        state: state.to_string(),
        compatibility_status,
        manifest_count: manifests.len() as u64,
        instance_ids,
        root_digest: base
            .and_then(|manifest| manifest.root.as_ref().map(|root| root.digest.clone())),
        manifest_digest: Some(manifest_set_digest(&manifests)),
        mismatch_count,
        divergent_range_count,
        divergent_row_count,
        stale_manifest_count,
        incompatible_manifest_count,
        unverifiable_count,
        diagnostic_sample: diagnostics.into_iter().take(16).collect(),
        last_error: if state == "consistent" {
            None
        } else {
            Some(state.to_string())
        },
    }
}

fn record_incompatibilities(
    base: &ProjectionVerificationManifest,
    manifest: &ProjectionVerificationManifest,
    diagnostics: &mut BTreeSet<String>,
) {
    if base.manifest_version != manifest.manifest_version {
        diagnostics.insert("manifest-version".to_string());
    }
    if base.projection_id != manifest.projection_id {
        diagnostics.insert("projection-id".to_string());
    }
    if base.projection_version_id != manifest.projection_version_id {
        diagnostics.insert("projection-version".to_string());
    }
    if base.schema_epoch != manifest.schema_epoch {
        diagnostics.insert("schema-epoch".to_string());
    }
    if base.projection_definition_hash != manifest.projection_definition_hash
        && (base.projection_definition_hash.is_some()
            || manifest.projection_definition_hash.is_some())
    {
        diagnostics.insert("projection-definition".to_string());
    }
    if base.source_identity != manifest.source_identity
        && (base.source_identity.is_some() || manifest.source_identity.is_some())
    {
        diagnostics.insert("source-identity".to_string());
    }
    if base.source_checkpoint != manifest.source_checkpoint
        && (base.source_checkpoint.is_some() || manifest.source_checkpoint.is_some())
    {
        diagnostics.insert("source-checkpoint".to_string());
    }
    if base.source_position != manifest.source_position
        && (base.source_position.is_some() || manifest.source_position.is_some())
    {
        diagnostics.insert("source-position".to_string());
    }
    if base.hash.algorithm != manifest.hash.algorithm {
        diagnostics.insert("hash-algorithm".to_string());
    }
    if base.hash.digest_length != manifest.hash.digest_length {
        diagnostics.insert("hash-digest-length".to_string());
    }
    if base.hash.canonical_encoder_version != manifest.hash.canonical_encoder_version {
        diagnostics.insert("hash-canonical-encoder".to_string());
    }
    if base.hash.row_hash_version != manifest.hash.row_hash_version {
        diagnostics.insert("row-hash-version".to_string());
    }
    if base.hash.range_hash_version != manifest.hash.range_hash_version {
        diagnostics.insert("range-hash-version".to_string());
    }
    if base.hash.root_hash_version != manifest.hash.root_hash_version {
        diagnostics.insert("root-hash-version".to_string());
    }
}

fn divergence_counts(
    manifests: &[ProjectionVerificationManifest],
    diagnostics: &mut BTreeSet<String>,
) -> (u64, u64) {
    let Some(base) = manifests.first() else {
        return (0, 0);
    };
    let base_ranges = base
        .ranges
        .iter()
        .map(|range| (range.range_id, range.digest.as_str()))
        .collect::<BTreeMap<_, _>>();
    let base_rows = base
        .row_hashes
        .iter()
        .map(|row| (row.row_id.as_str(), row.digest.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut divergent_ranges = BTreeSet::new();
    let mut divergent_rows = BTreeSet::new();
    for manifest in manifests.iter().skip(1) {
        let range_ids = base_ranges
            .keys()
            .copied()
            .chain(manifest.ranges.iter().map(|range| range.range_id))
            .collect::<BTreeSet<_>>();
        for range_id in range_ids {
            let other = manifest
                .ranges
                .iter()
                .find(|range| range.range_id == range_id)
                .map(|range| range.digest.as_str());
            if base_ranges.get(&range_id).copied() != other {
                divergent_ranges.insert(range_id);
                diagnostics.insert(format!("range:{range_id}"));
            }
        }

        let row_ids = base_rows
            .keys()
            .copied()
            .chain(manifest.row_hashes.iter().map(|row| row.row_id.as_str()))
            .collect::<BTreeSet<_>>();
        for row_id in row_ids {
            let other = manifest
                .row_hashes
                .iter()
                .find(|row| row.row_id == row_id)
                .map(|row| row.digest.as_str());
            if base_rows.get(row_id).copied() != other {
                divergent_rows.insert(row_id.to_string());
                diagnostics.insert(format!("row:{row_id}"));
            }
        }
    }
    (divergent_ranges.len() as u64, divergent_rows.len() as u64)
}

fn manifest_is_stale(manifest: &ProjectionVerificationManifest, now_ms: u64) -> bool {
    manifest.expires_at_ms <= now_ms
        || manifest
            .root
            .as_ref()
            .is_some_and(|root| root.state == "stale")
}

fn manifest_is_unverifiable(manifest: &ProjectionVerificationManifest) -> bool {
    manifest
        .root
        .as_ref()
        .is_none_or(|root| root.digest.is_empty())
        || manifest.hash.algorithm.is_empty()
        || manifest
            .root
            .as_ref()
            .is_some_and(|root| !matches!(root.state.as_str(), "current" | "empty" | "stale"))
}

fn root_hash_metadata(
    root: &crate::midge::adapter::RootHashRecord,
) -> ProjectionManifestHashMetadata {
    ProjectionManifestHashMetadata {
        algorithm: root.algorithm.clone(),
        digest_length: root.digest_length,
        canonical_encoder_version: root.canonical_encoder_version,
        row_hash_version: root.row_hash_version,
        range_hash_version: root.range_hash_version,
        root_hash_version: root.root_hash_version,
    }
}

fn root_summary(root: &crate::midge::adapter::RootHashRecord) -> ProjectionManifestRootSummary {
    ProjectionManifestRootSummary {
        digest: root.digest.clone(),
        row_count: root.row_count,
        range_count: root.range_count,
        state: stored_hash_state(&root.state).to_string(),
        computed_ms: root.computed_ms,
    }
}

fn range_summary(range: &crate::midge::adapter::RangeHashRecord) -> ProjectionManifestRangeSummary {
    ProjectionManifestRangeSummary {
        range_id: range.range_id,
        first_row_id: range.first_row_id.clone(),
        last_row_id: range.last_row_id.clone(),
        row_count: range.row_count,
        digest: range.digest.clone(),
        state: stored_hash_state(&range.state).to_string(),
        computed_ms: range.computed_ms,
    }
}

fn row_hash_summaries(
    midge: &crate::midge::adapter::Midge,
    collection: &str,
) -> Result<Vec<ProjectionManifestRowHashSummary>, CassieError> {
    let mut rows = midge.list_row_hashes(collection)?;
    rows.sort_by(|left, right| left.row_id.cmp(&right.row_id));
    Ok(rows
        .into_iter()
        .map(|row| ProjectionManifestRowHashSummary {
            row_id: row.row_id,
            digest: row.digest,
            state: stored_hash_state(&row.state).to_string(),
            computed_ms: row.computed_ms,
        })
        .collect())
}

fn stored_hash_state(state: &crate::midge::adapter::StoredHashState) -> &'static str {
    match state {
        crate::midge::adapter::StoredHashState::Current => "current",
        crate::midge::adapter::StoredHashState::Stale => "stale",
        crate::midge::adapter::StoredHashState::Incomplete => "incomplete",
        crate::midge::adapter::StoredHashState::Incompatible => "incompatible",
        crate::midge::adapter::StoredHashState::Empty => "empty",
        crate::midge::adapter::StoredHashState::Tombstone => "missing",
    }
}

fn manifest_set_digest(manifests: &[ProjectionVerificationManifest]) -> String {
    let parts = manifests
        .iter()
        .map(|manifest| format!("{}:{}", manifest.instance_id, manifest.manifest_digest))
        .collect::<Vec<_>>();
    stable_digest(&parts)
}

fn manifest_digest(manifest: &ProjectionVerificationManifest) -> String {
    let mut canonical = manifest.clone();
    canonical.manifest_digest.clear();
    stable_digest(&canonical)
}

fn stable_digest<T: serde::Serialize>(value: &T) -> String {
    let mut writer = StableDigestWriter::default();
    serde_json::to_writer(&mut writer, value).expect("serialize manifest digest");
    format!("fnv64:{:016x}", writer.finish())
}

#[derive(Default)]
struct StableDigestWriter {
    state: u64,
}

impl StableDigestWriter {
    fn finish(&self) -> u64 {
        self.state
    }
}

impl Write for StableDigestWriter {
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
