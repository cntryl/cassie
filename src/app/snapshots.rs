use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::*;

const SNAPSHOT_FORMAT_VERSION: u16 = 1;
const SNAPSHOT_MANIFEST_FILE: &str = "cassie-snapshot-manifest.json";
const SNAPSHOT_MIDGE_DIR: &str = "midge";
const SNAPSHOT_COMPATIBLE: &str = "compatible";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassieSnapshotOptions {
    pub generated_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassieSnapshotManifest {
    pub format_version: u16,
    pub cassie_version: String,
    pub generated_ms: u64,
    pub schema_epoch: u64,
    pub compatibility_status: String,
    pub midge_data_path: String,
    pub projections: Vec<CassieSnapshotProjectionManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassieSnapshotProjectionManifest {
    pub projection_id: String,
    pub projection_kind: String,
    pub collection: String,
    pub schema_version: u32,
    pub active_version: Option<String>,
    pub source_identity: Option<String>,
    pub source_checkpoint: Option<String>,
    pub source_position: Option<u64>,
    pub hash: CassieSnapshotHashManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassieSnapshotHashManifest {
    pub algorithm: String,
    pub digest_length: u16,
    pub canonical_encoder_version: u16,
    pub row_hash_version: u16,
    pub range_hash_version: u16,
    pub root_hash_version: u16,
    pub root_digest: Option<String>,
    pub root_state: String,
    pub row_count: u64,
    pub range_count: u64,
}

impl Cassie {
    pub fn create_snapshot_from_data_dir(
        data_dir: impl AsRef<Path>,
        snapshot_dir: impl AsRef<Path>,
        options: CassieSnapshotOptions,
    ) -> Result<CassieSnapshotManifest, CassieError> {
        let data_dir = data_dir.as_ref();
        let snapshot_dir = snapshot_dir.as_ref();
        if !data_dir.is_dir() {
            return Err(CassieError::NotFound(format!(
                "snapshot source data directory not found: {}",
                data_dir.display()
            )));
        }
        if snapshot_dir.starts_with(data_dir) {
            return Err(CassieError::Unsupported(
                "snapshot directory must not be inside the source data directory".to_string(),
            ));
        }

        prepare_empty_directory(snapshot_dir, "snapshot")?;
        let manifest = {
            let midge = Midge::new_strict_with_data_dir(data_dir)?;
            midge.ensure_families_ready()?;
            build_snapshot_manifest(&midge, options)?
        };
        copy_dir_recursive(data_dir, &snapshot_dir.join(SNAPSHOT_MIDGE_DIR))?;
        write_snapshot_manifest(snapshot_dir, &manifest)?;
        Ok(manifest)
    }

    pub fn restore_snapshot(
        snapshot_dir: impl AsRef<Path>,
        target_data_dir: impl AsRef<Path>,
    ) -> Result<CassieSnapshotManifest, CassieError> {
        let snapshot_dir = snapshot_dir.as_ref();
        let target_data_dir = target_data_dir.as_ref();
        let manifest = read_snapshot_manifest(snapshot_dir)?;
        validate_snapshot_manifest(&manifest)?;
        let snapshot_midge_dir = snapshot_dir.join(&manifest.midge_data_path);
        if !snapshot_midge_dir.is_dir() {
            return Err(CassieError::NotFound(format!(
                "snapshot midge data directory not found: {}",
                snapshot_midge_dir.display()
            )));
        }

        prepare_restore_target(target_data_dir)?;
        copy_dir_recursive(&snapshot_midge_dir, target_data_dir)?;
        Ok(manifest)
    }
}

fn build_snapshot_manifest(
    midge: &Midge,
    options: CassieSnapshotOptions,
) -> Result<CassieSnapshotManifest, CassieError> {
    let schema_epoch = midge.schema_epoch()?;
    let mut projections = midge
        .list_projection_metadata()?
        .into_iter()
        .map(|metadata| snapshot_projection_manifest(midge, metadata))
        .collect::<Result<Vec<_>, _>>()?;
    projections.sort_by(|left, right| {
        left.projection_id
            .cmp(&right.projection_id)
            .then_with(|| left.collection.cmp(&right.collection))
    });

    Ok(CassieSnapshotManifest {
        format_version: SNAPSHOT_FORMAT_VERSION,
        cassie_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_ms: options.generated_ms.unwrap_or_else(current_time_millis),
        schema_epoch,
        compatibility_status: SNAPSHOT_COMPATIBLE.to_string(),
        midge_data_path: SNAPSHOT_MIDGE_DIR.to_string(),
        projections,
    })
}

fn snapshot_projection_manifest(
    midge: &Midge,
    metadata: crate::catalog::ProjectionMeta,
) -> Result<CassieSnapshotProjectionManifest, CassieError> {
    let hash_collection = metadata
        .active_output_collection()
        .unwrap_or(&metadata.collection);
    Ok(CassieSnapshotProjectionManifest {
        projection_id: metadata.projection_id().to_string(),
        projection_kind: metadata.kind.as_str().to_string(),
        collection: metadata.collection.clone(),
        schema_version: metadata.schema_version,
        active_version: metadata.active_version.clone(),
        source_identity: metadata.source_identity.clone(),
        source_checkpoint: metadata.source_checkpoint.clone(),
        source_position: metadata.source_position,
        hash: snapshot_hash_manifest(midge, hash_collection, &metadata)?,
    })
}

fn snapshot_hash_manifest(
    midge: &Midge,
    collection: &str,
    metadata: &crate::catalog::ProjectionMeta,
) -> Result<CassieSnapshotHashManifest, CassieError> {
    let root = midge.root_hash(collection)?;
    if let Some(root) = root {
        return Ok(CassieSnapshotHashManifest {
            algorithm: root.algorithm,
            digest_length: root.digest_length,
            canonical_encoder_version: root.canonical_encoder_version,
            row_hash_version: root.row_hash_version,
            range_hash_version: root.range_hash_version,
            root_hash_version: root.root_hash_version,
            root_digest: Some(root.digest),
            root_state: stored_hash_state(&root.state).to_string(),
            row_count: root.row_count,
            range_count: root.range_count,
        });
    }

    Ok(CassieSnapshotHashManifest {
        algorithm: metadata.hashes.algorithm.algorithm.clone(),
        digest_length: metadata.hashes.algorithm.digest_length,
        canonical_encoder_version: metadata.hashes.algorithm.canonical_encoder_version,
        row_hash_version: metadata.hashes.algorithm.hash_version,
        range_hash_version: metadata.hashes.algorithm.hash_version,
        root_hash_version: metadata.hashes.algorithm.hash_version,
        root_digest: metadata.hashes.root.digest.clone(),
        root_state: metadata.hashes.root.state.as_str().to_string(),
        row_count: metadata.hashes.root.row_count,
        range_count: metadata.hashes.root.range_count,
    })
}

fn stored_hash_state(state: &crate::midge::adapter::StoredHashState) -> &'static str {
    match state {
        crate::midge::adapter::StoredHashState::Current => "current",
        crate::midge::adapter::StoredHashState::Stale => "stale",
        crate::midge::adapter::StoredHashState::Incomplete => "incomplete",
        crate::midge::adapter::StoredHashState::Incompatible => "incompatible",
        crate::midge::adapter::StoredHashState::Empty => "empty",
        crate::midge::adapter::StoredHashState::Tombstone => "tombstone",
    }
}

fn validate_snapshot_manifest(manifest: &CassieSnapshotManifest) -> Result<(), CassieError> {
    if manifest.format_version != SNAPSHOT_FORMAT_VERSION {
        return Err(CassieError::Storage(format!(
            "snapshot manifest version {} is unsupported; expected {}",
            manifest.format_version, SNAPSHOT_FORMAT_VERSION
        )));
    }
    if manifest.cassie_version != env!("CARGO_PKG_VERSION") {
        return Err(CassieError::Storage(format!(
            "snapshot cassie version '{}' is incompatible with '{}'",
            manifest.cassie_version,
            env!("CARGO_PKG_VERSION")
        )));
    }
    if manifest.compatibility_status != SNAPSHOT_COMPATIBLE {
        return Err(CassieError::Storage(format!(
            "snapshot compatibility status '{}' is not compatible",
            manifest.compatibility_status
        )));
    }
    if manifest.midge_data_path != SNAPSHOT_MIDGE_DIR {
        return Err(CassieError::Storage(format!(
            "snapshot midge data path '{}' is unsupported",
            manifest.midge_data_path
        )));
    }
    for projection in &manifest.projections {
        if projection.hash.algorithm.trim().is_empty() {
            return Err(CassieError::Storage(format!(
                "snapshot projection '{}' has empty hash algorithm",
                projection.projection_id
            )));
        }
        if projection.hash.digest_length == 0 {
            return Err(CassieError::Storage(format!(
                "snapshot projection '{}' has invalid hash digest length",
                projection.projection_id
            )));
        }
    }
    Ok(())
}

fn write_snapshot_manifest(
    snapshot_dir: &Path,
    manifest: &CassieSnapshotManifest,
) -> Result<(), CassieError> {
    let encoded = serde_json::to_vec_pretty(manifest)
        .map_err(|error| CassieError::Storage(format!("encode snapshot manifest: {error}")))?;
    fs::write(snapshot_dir.join(SNAPSHOT_MANIFEST_FILE), encoded)
        .map_err(|error| io_error("write snapshot manifest", error))
}

fn read_snapshot_manifest(snapshot_dir: &Path) -> Result<CassieSnapshotManifest, CassieError> {
    let path = snapshot_dir.join(SNAPSHOT_MANIFEST_FILE);
    let raw = fs::read(&path).map_err(|error| io_error("read snapshot manifest", error))?;
    serde_json::from_slice(&raw)
        .map_err(|error| CassieError::Storage(format!("invalid snapshot manifest: {error}")))
}

fn prepare_empty_directory(path: &Path, label: &str) -> Result<(), CassieError> {
    if path.exists() {
        if !path.is_dir() {
            return Err(CassieError::Storage(format!(
                "{label} path is not a directory: {}",
                path.display()
            )));
        }
        if path
            .read_dir()
            .map_err(|error| io_error("read directory", error))?
            .next()
            .is_some()
        {
            return Err(CassieError::Storage(format!(
                "{label} directory must be empty: {}",
                path.display()
            )));
        }
        return Ok(());
    }
    fs::create_dir_all(path).map_err(|error| io_error("create directory", error))
}

fn prepare_restore_target(path: &Path) -> Result<(), CassieError> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        return Err(CassieError::Storage(format!(
            "restore target is not a directory: {}",
            path.display()
        )));
    }
    if path
        .read_dir()
        .map_err(|error| io_error("read restore target", error))?
        .next()
        .is_some()
    {
        return Err(CassieError::Storage(format!(
            "restore target directory must be empty: {}",
            path.display()
        )));
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), CassieError> {
    if !source.is_dir() {
        return Err(CassieError::NotFound(format!(
            "source directory not found: {}",
            source.display()
        )));
    }
    fs::create_dir_all(target).map_err(|error| io_error("create directory", error))?;
    for entry in fs::read_dir(source).map_err(|error| io_error("read directory", error))? {
        let entry = entry.map_err(|error| io_error("read directory entry", error))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("read file type", error))?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|error| io_error("copy snapshot file", error))?;
        } else {
            return Err(CassieError::Unsupported(format!(
                "snapshot copy does not support special file: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

fn io_error(operation: &str, error: std::io::Error) -> CassieError {
    CassieError::Storage(format!("{operation}: {error}"))
}
