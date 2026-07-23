use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{current_time_millis, Cassie, CassieError, Midge};

const SNAPSHOT_FORMAT_VERSION: u16 = 2;
const SNAPSHOT_MANIFEST_FILE: &str = "cassie-snapshot-manifest.json";
const SNAPSHOT_MIDGE_DIR: &str = "midge";
const SNAPSHOT_COMPATIBLE: &str = "compatible";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassieSnapshotOptions {
    pub generated_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassieSnapshotManifest {
    pub format_version: u16,
    pub cassie_version: String,
    pub generated_ms: u64,
    pub schema_epoch: u64,
    pub data_epoch: u64,
    pub compatibility_status: String,
    pub midge_data_path: String,
    pub projections: Vec<CassieSnapshotProjectionManifest>,
    pub collections: Vec<CassieSnapshotCollectionManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassieSnapshotCollectionManifest {
    pub collection: String,
    pub generation: u64,
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
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn create_snapshot_from_data_dir(
        data_dir: impl AsRef<Path>,
        snapshot_dir: impl AsRef<Path>,
        options: CassieSnapshotOptions,
    ) -> Result<CassieSnapshotManifest, CassieError> {
        Self::create_snapshot_with_copy_hook(
            data_dir.as_ref(),
            snapshot_dir.as_ref(),
            options,
            |_| Ok(()),
        )
    }

    /// # Errors
    ///
    /// Returns an error when the snapshot source is invalid, copying fails, or source generation
    /// changes during the copy.
    fn create_snapshot_with_copy_hook<F>(
        data_dir: &Path,
        snapshot_dir: &Path,
        options: CassieSnapshotOptions,
        during_copy: F,
    ) -> Result<CassieSnapshotManifest, CassieError>
    where
        F: FnOnce(&Path) -> Result<(), CassieError>,
    {
        if !data_dir.is_dir() {
            return Err(CassieError::NotFound(format!(
                "snapshot source data directory not found: {}",
                data_dir.display()
            )));
        }
        if paths_overlap(data_dir, snapshot_dir)? {
            return Err(CassieError::Unsupported(
                "snapshot source and destination directories must not overlap".to_string(),
            ));
        }

        prepare_empty_directory(snapshot_dir, "snapshot")?;
        let result = (|| {
            let manifest = {
                let midge = Midge::new_strict_with_data_dir(data_dir)?;
                midge.ensure_families_ready()?;
                build_snapshot_manifest(&midge, options)?
            };
            let copied_midge_dir = snapshot_dir.join(SNAPSHOT_MIDGE_DIR);
            let mut copy_hook = Some(during_copy);
            let mut during_copy = |_: &Path| copy_hook.take().map_or(Ok(()), |hook| hook(data_dir));
            copy_dir_recursive_with_hook(data_dir, &copied_midge_dir, &mut during_copy)?;
            let copied_generation = {
                let midge = Midge::new_strict_with_data_dir(data_dir)?;
                midge.ensure_families_ready()?;
                build_snapshot_manifest(&midge, options)?
            };
            if !same_snapshot_generation(&manifest, &copied_generation) {
                return Err(CassieError::Storage(
                    "snapshot source changed while copying; retry snapshot".to_string(),
                ));
            }
            write_snapshot_manifest(snapshot_dir, &manifest)?;
            Ok(manifest)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(snapshot_dir);
        }
        result
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn restore_snapshot(
        snapshot_dir: impl AsRef<Path>,
        target_data_dir: impl AsRef<Path>,
    ) -> Result<CassieSnapshotManifest, CassieError> {
        let snapshot_dir = snapshot_dir.as_ref();
        let target_data_dir = target_data_dir.as_ref();
        if paths_overlap(snapshot_dir, target_data_dir)? {
            return Err(CassieError::Unsupported(
                "snapshot source and restore target directories must not overlap".to_string(),
            ));
        }
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
        let result = (|| {
            copy_dir_recursive(&snapshot_midge_dir, target_data_dir)?;
            validate_restored_snapshot_state(target_data_dir, &manifest)?;
            Ok(manifest.clone())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(target_data_dir);
        }
        result
    }
}

fn paths_overlap(left: &Path, right: &Path) -> Result<bool, CassieError> {
    let left = canonical_path_candidate(left)?;
    let right = canonical_path_candidate(right)?;
    Ok(left.starts_with(&right) || right.starts_with(&left))
}

fn canonical_path_candidate(path: &Path) -> Result<std::path::PathBuf, CassieError> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|error| {
            CassieError::Storage(format!("canonicalize path '{}': {error}", path.display()))
        });
    }
    let parent = path.parent().ok_or_else(|| {
        CassieError::Parse(format!("path '{}' has no parent directory", path.display()))
    })?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        CassieError::Storage(format!(
            "canonicalize parent directory '{}': {error}",
            parent.display()
        ))
    })?;
    let name = path.file_name().ok_or_else(|| {
        CassieError::Parse(format!("path '{}' has no final component", path.display()))
    })?;
    Ok(canonical_parent.join(name))
}

fn build_snapshot_manifest(
    midge: &Midge,
    options: CassieSnapshotOptions,
) -> Result<CassieSnapshotManifest, CassieError> {
    let schema_epoch = midge.schema_epoch()?;
    let data_epoch = midge.data_epoch()?;
    let mut projections = midge
        .list_projection_metadata()?
        .into_iter()
        .map(|metadata| snapshot_projection_manifest(midge, &metadata))
        .collect::<Result<Vec<_>, _>>()?;
    projections.sort_by(|left, right| {
        left.projection_id
            .cmp(&right.projection_id)
            .then_with(|| left.collection.cmp(&right.collection))
    });
    let mut collections = midge
        .list_collections()
        .into_iter()
        .map(|collection| {
            Ok(CassieSnapshotCollectionManifest {
                generation: midge.collection_generation(&collection)?,
                collection,
            })
        })
        .collect::<Result<Vec<_>, CassieError>>()?;
    collections.sort_by(|left, right| left.collection.cmp(&right.collection));

    Ok(CassieSnapshotManifest {
        format_version: SNAPSHOT_FORMAT_VERSION,
        cassie_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_ms: options.generated_ms.unwrap_or_else(current_time_millis),
        schema_epoch,
        data_epoch,
        compatibility_status: SNAPSHOT_COMPATIBLE.to_string(),
        midge_data_path: SNAPSHOT_MIDGE_DIR.to_string(),
        projections,
        collections,
    })
}

fn same_snapshot_generation(
    before: &CassieSnapshotManifest,
    after: &CassieSnapshotManifest,
) -> bool {
    before.schema_epoch == after.schema_epoch
        && before.data_epoch == after.data_epoch
        && before.collections == after.collections
        && before.projections == after.projections
}

fn validate_restored_snapshot_state(
    target_data_dir: &Path,
    manifest: &CassieSnapshotManifest,
) -> Result<(), CassieError> {
    let midge = Midge::new_strict_with_data_dir(target_data_dir)?;
    midge.ensure_families_ready()?;
    midge.validate_recovery_state()?;
    let restored = build_snapshot_manifest(
        &midge,
        CassieSnapshotOptions {
            generated_ms: Some(manifest.generated_ms),
        },
    )?;
    if restored.schema_epoch != manifest.schema_epoch {
        return Err(CassieError::Storage(format!(
            "snapshot schema epoch does not match restored schema: expected {}, got {}",
            manifest.schema_epoch, restored.schema_epoch
        )));
    }
    if restored.data_epoch != manifest.data_epoch {
        return Err(CassieError::Storage(format!(
            "snapshot data epoch does not match restored data: expected {}, got {}",
            manifest.data_epoch, restored.data_epoch
        )));
    }
    if restored.collections != manifest.collections {
        return Err(CassieError::Storage(
            "snapshot collection generations do not match restored data".to_string(),
        ));
    }
    if restored.projections != manifest.projections {
        return Err(CassieError::Storage(
            "snapshot projection state does not match restored data".to_string(),
        ));
    }
    Ok(())
}

fn snapshot_projection_manifest(
    midge: &Midge,
    metadata: &crate::catalog::ProjectionMeta,
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
        hash: snapshot_hash_manifest(midge, hash_collection, metadata)?,
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
        .map_err(|error| io_error("write snapshot manifest", &error))
}

fn read_snapshot_manifest(snapshot_dir: &Path) -> Result<CassieSnapshotManifest, CassieError> {
    let path = snapshot_dir.join(SNAPSHOT_MANIFEST_FILE);
    let raw = fs::read(&path).map_err(|error| io_error("read snapshot manifest", &error))?;
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
            .map_err(|error| io_error("read directory", &error))?
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
    fs::create_dir_all(path).map_err(|error| io_error("create directory", &error))
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
        .map_err(|error| io_error("read restore target", &error))?
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
    let mut no_hook = |_path: &Path| Ok(());
    copy_dir_recursive_with_hook(source, target, &mut no_hook)
}

fn copy_dir_recursive_with_hook(
    source: &Path,
    target: &Path,
    hook: &mut dyn FnMut(&Path) -> Result<(), CassieError>,
) -> Result<(), CassieError> {
    if !source.is_dir() {
        return Err(CassieError::NotFound(format!(
            "source directory not found: {}",
            source.display()
        )));
    }
    fs::create_dir_all(target).map_err(|error| io_error("create directory", &error))?;
    for entry in fs::read_dir(source).map_err(|error| io_error("read directory", &error))? {
        let entry = entry.map_err(|error| io_error("read directory entry", &error))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("read file type", &error))?;
        if file_type.is_dir() {
            copy_dir_recursive_with_hook(&source_path, &target_path, hook)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|error| io_error("copy snapshot file", &error))?;
            hook(&source_path)?;
        } else {
            return Err(CassieError::Unsupported(format!(
                "snapshot copy does not support special file: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

fn io_error(operation: &str, error: &std::io::Error) -> CassieError {
    CassieError::Storage(format!("{operation}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_invoke_snapshot_copy_hook_during_recursive_copy() {
        // Arrange
        let source = std::env::temp_dir().join(format!(
            "cassie-snapshot-hook-source-{}",
            uuid::Uuid::new_v4()
        ));
        let target = std::env::temp_dir().join(format!(
            "cassie-snapshot-hook-target-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&source).expect("create hook source");
        fs::write(source.join("first-file"), b"first").expect("seed first file");
        fs::write(source.join("second-file"), b"second").expect("seed second file");
        let mut hook_calls = 0usize;

        // Act
        copy_dir_recursive_with_hook(&source, &target, &mut |_path| {
            hook_calls += 1;
            Ok(())
        })
        .expect("copy source");

        // Assert
        assert_eq!(hook_calls, 2);
        assert_eq!(fs::read(target.join("first-file")).unwrap(), b"first");
        assert_eq!(fs::read(target.join("second-file")).unwrap(), b"second");

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(target);
    }

    #[test]
    fn should_reject_snapshot_when_source_mutates_during_copy() {
        // Arrange
        let source = std::env::temp_dir().join(format!(
            "cassie-snapshot-mutation-source-{}",
            uuid::Uuid::new_v4()
        ));
        let snapshot = std::env::temp_dir().join(format!(
            "cassie-snapshot-mutation-bundle-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie = Cassie::new_with_data_dir(&source).expect("create source Cassie");
        cassie.startup().expect("start source Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE snapshot_mutation_docs (title TEXT)",
                Vec::new(),
            )
            .expect("create source table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO snapshot_mutation_docs (title) VALUES ('before')",
                Vec::new(),
            )
            .expect("seed source table");
        drop(cassie);

        let (start_writer, writer_started) = std::sync::mpsc::channel();
        let (writer_finished, writer_done) = std::sync::mpsc::channel();
        let (copy_continue, wait_for_copy) = std::sync::mpsc::channel();
        let writer_source = source.clone();
        let writer = std::thread::spawn(move || {
            writer_started.recv().expect("start snapshot writer");
            let cassie = Cassie::new_with_data_dir(&writer_source).expect("open writer Cassie");
            cassie.startup().expect("start writer Cassie");
            let session = cassie.create_session("writer", None);
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO snapshot_mutation_docs (title) VALUES ('during-copy')",
                    Vec::new(),
                )
                .expect("mutate source during copy");
            drop(cassie);
            writer_finished.send(()).expect("report source mutation");
            wait_for_copy.recv().expect("finish snapshot copy");
        });

        // Act
        let error = Cassie::create_snapshot_with_copy_hook(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(7_913),
            },
            |_source| {
                start_writer
                    .send(())
                    .map_err(|error| CassieError::Storage(error.to_string()))?;
                writer_done
                    .recv()
                    .map_err(|error| CassieError::Storage(error.to_string()))?;
                copy_continue
                    .send(())
                    .map_err(|error| CassieError::Storage(error.to_string()))
            },
        )
        .expect_err("source mutation must invalidate snapshot");
        writer.join().expect("join snapshot writer");

        // Assert
        assert!(error
            .to_string()
            .contains("snapshot source changed while copying"));
        assert!(!snapshot.exists());

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(snapshot);
    }
}
