//! Private disk storage for one vector product run.

use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

#[path = "../fixture_dir.rs"]
mod fixture_dir;

pub(super) struct ProductScratch {
    path: PathBuf,
    _cleanup: fixture_dir::FixtureDir,
}

impl ProductScratch {
    pub(super) fn create(
        temporary_root: &Path,
        shard_output: &Path,
        artifact_output: Option<&Path>,
    ) -> io::Result<Self> {
        let temporary_root = temporary_root.canonicalize()?;
        for published_output in std::iter::once(shard_output).chain(artifact_output) {
            fs::create_dir_all(published_output)?;
            if temporary_root.starts_with(published_output.canonicalize()?) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "vector product scratch must be outside published output",
                ));
            }
        }

        let path = temporary_root.join(format!("cassie-vector-product-{}", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        #[cfg(not(unix))]
        fs::create_dir(&path)?;
        let cleanup = fixture_dir::FixtureDir::new(&path);
        Ok(Self {
            path,
            _cleanup: cleanup,
        })
    }

    pub(super) fn work_dir(&self) -> PathBuf {
        self.path.join("work")
    }

    pub(super) fn truth_cache_dir(&self) -> PathBuf {
        self.path.join("truth-cache")
    }
}
