//! Pure receipt verification and owned-copy boundary; never opens an engine.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SourceIdentity {
    pub(crate) commit: String,
    pub(crate) tree: String,
}
impl SourceIdentity {
    pub(crate) fn validate(&self) -> Result<()> {
        if [self.commit.as_str(), self.tree.as_str()]
            .iter()
            .all(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            Ok(())
        } else {
            Err("source identities require full commit/tree hashes".into())
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ProfileFiles {
    pub(crate) result_path: PathBuf,
    pub(crate) result_sha256: String,
    pub(crate) log_path: PathBuf,
    pub(crate) log_sha256: String,
    pub(crate) closure_path: PathBuf,
    pub(crate) closure_sha256: String,
    pub(crate) source_fingerprint: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ProducerResult {
    pub(crate) source: String,
    pub(crate) mode: String,
    pub(crate) shape: String,
    pub(crate) workload: String,
    pub(crate) batch_size: usize,
    pub(crate) batch_count: usize,
    pub(crate) events: usize,
    pub(crate) query_memory_budget_bytes: usize,
    pub(crate) query_timeout_ms: u64,
    pub(crate) storage_path: PathBuf,
    pub(crate) recovery_boundary: String,
}

#[derive(Deserialize)]
struct Closure {
    writer: SourceIdentity,
    exit_code: i32,
    clean_closed: bool,
    storage_path: PathBuf,
    result_sha256: String,
    log_sha256: String,
}

#[derive(Debug)]
pub(crate) struct VerifiedSource {
    pub(crate) result: ProducerResult,
    pub(crate) writer: SourceIdentity,
    pub(crate) files: ProfileFiles,
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "symlink/non-file receipt or content rejected: {}",
            path.display()
        ));
    }
    let mut file = File::open(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut hash = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    loop {
        let size = file.read(&mut bytes).map_err(|error| error.to_string())?;
        if size == 0 {
            break;
        }
        hash.update(&bytes[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn check_hash(path: &Path, expected: &str) -> Result<()> {
    if expected.len() != 64
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        || sha256_file(path)? != expected
    {
        return Err(format!("receipt SHA-256 mismatch: {}", path.display()));
    }
    Ok(())
}

pub(crate) fn load_profile(
    files: &ProfileFiles,
    api: &SourceIdentity,
    sql: &SourceIdentity,
) -> Result<VerifiedSource> {
    api.validate()?;
    sql.validate()?;
    check_hash(&files.result_path, &files.result_sha256)?;
    check_hash(&files.log_path, &files.log_sha256)?;
    check_hash(&files.closure_path, &files.closure_sha256)?;
    let result: ProducerResult =
        serde_json::from_reader(File::open(&files.result_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("producer result: {error}"))?;
    let closure: Closure = serde_json::from_reader(
        File::open(&files.closure_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("producer closure: {error}"))?;
    let writer = match result.mode.as_str() {
        "local_strict" | "local_buffered" => api,
        "sql_default" | "sql_replayable" => sql,
        _ => return Err("foreign producer mode".into()),
    };
    if closure.writer != *writer
        || !result
            .source
            .starts_with(&format!("{};tree={};", writer.commit, writer.tree))
    {
        return Err("foreign producer commit/tree".into());
    }
    if closure.exit_code != 0
        || !closure.clean_closed
        || closure.storage_path != result.storage_path
        || closure.result_sha256 != files.result_sha256
        || closure.log_sha256 != files.log_sha256
    {
        return Err("missing or mismatched successful clean-close evidence".into());
    }
    if result.workload != "apply"
        || !["indexed", "unindexed"].contains(&result.shape.as_str())
        || result.batch_size != 1000
        || result.batch_count != 100
        || result.events != 100_000
        || result.query_memory_budget_bytes != 64 * 1024 * 1024
        || result.query_timeout_ms != 120_000
        || result.recovery_boundary
            != "clean shutdown then constructor and startup; verification excluded"
    {
        return Err("producer is outside the selected 100k/1000x100 closed-store profile".into());
    }
    Ok(VerifiedSource {
        result,
        writer: writer.clone(),
        files: files.clone(),
    })
}

fn entries(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    paths.sort();
    Ok(paths)
}

fn inspect(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("symlink rejected: {}", path.display()));
    }
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(format!("non-regular source rejected: {}", path.display()));
    }
    Ok(metadata)
}

fn hash_tree(root: &Path, directory: &Path, hash: &mut Sha256) -> Result<()> {
    for path in entries(directory)? {
        let metadata = inspect(&path)?;
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_str()
            .ok_or("non-Unicode store filename")?
            .to_owned();
        if metadata.is_dir() {
            hash.update(format!("dir\0{relative}\n").as_bytes());
            hash_tree(root, &path, hash)?;
        } else {
            hash.update(
                format!(
                    "file\0{relative}\0{}\0{}\n",
                    metadata.len(),
                    sha256_file(&path)?
                )
                .as_bytes(),
            );
        }
    }
    Ok(())
}

/// Claim the actual original directory before copying any selected store.
pub(crate) fn claim_original(
    path: &Path,
    originals: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<()> {
    if !inspect(path)?.is_dir() {
        return Err("original must be a directory".into());
    }
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if !originals.insert(canonical) {
        return Err("duplicate canonical original store".into());
    }
    Ok(())
}

/// Fingerprint relative paths, entry types, lengths and file bytes; not access times.
pub(crate) fn fingerprint(root: &Path) -> Result<String> {
    if !inspect(root)?.is_dir() {
        return Err("store source must be a directory".into());
    }
    let mut hash = Sha256::new();
    hash_tree(root, root, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for path in entries(source)? {
        let metadata = inspect(&path)?;
        let target = destination.join(path.file_name().ok_or("source filename")?);
        if metadata.is_dir() {
            fs::create_dir(&target).map_err(|error| error.to_string())?;
            copy_tree(&path, &target)?;
        } else {
            let mut input = File::open(&path).map_err(|error| error.to_string())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|error| error.to_string())?;
            std::io::copy(&mut input, &mut output).map_err(|error| error.to_string())?;
            output.flush().map_err(|error| error.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let copied = output.metadata().map_err(|error| error.to_string())?;
                if metadata.dev() == copied.dev() && metadata.ino() == copied.ino() {
                    return Err("copy unexpectedly aliases original file".into());
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct OwnedCopy {
    directory: PathBuf,
    original: PathBuf,
    expected: String,
}
impl OwnedCopy {
    pub(crate) fn prepare(source: &VerifiedSource) -> Result<Self> {
        let expected = &source.files.source_fingerprint;
        if fingerprint(&source.result.storage_path)? != *expected {
            return Err("original fingerprint mismatch before copy".into());
        }
        let original = source
            .result
            .storage_path
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let parent = std::env::temp_dir()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if parent.starts_with(&original) {
            return Err("source cannot contain the owned copy root".into());
        }
        let directory = parent.join(format!("cassie-replay-read-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let owned = Self {
            directory,
            original,
            expected: expected.clone(),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&owned.directory, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        copy_tree(&owned.original, &owned.directory)?;
        if fingerprint(&owned.directory)? != *expected {
            return Err("owned copy content does not match original fingerprint".into());
        }
        owned.verify_original()?;
        Ok(owned)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.directory
    }
    pub(crate) fn verify_original(&self) -> Result<()> {
        if fingerprint(&self.original)? != self.expected {
            return Err("original source changed during read qualification".into());
        }
        Ok(())
    }
}
impl Drop for OwnedCopy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
