//! Downloads, verifies and extracts the Cranfield archive for `relevance_cranfield` (#1368).
//!
//! The collection has no published license and is never committed. The archive is cached under
//! `target/relevance/cranfield/` and every use is gated on the SHA-256 and size in the manifest.
//! Extraction shells out to the system `tar` and names only the three required members, so the
//! workspace gains no archive dependencies and nothing else in the archive is written.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::cranfield::Manifest;

pub struct Collection {
    pub documents: String,
    pub queries: String,
    pub qrels: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            write!(out, "{byte:02x}").expect("write to string");
            out
        })
}

fn verify(manifest: &Manifest, bytes: &[u8]) -> Result<(), String> {
    let actual = sha256_hex(bytes);
    if bytes.len() as u64 != manifest.source.archive_bytes
        || actual != manifest.source.archive_sha256
    {
        return Err(format!(
            "archive mismatch: expected {} bytes and sha256 {}, got {} bytes and sha256 {actual}",
            manifest.source.archive_bytes,
            manifest.source.archive_sha256,
            bytes.len()
        ));
    }
    Ok(())
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| format!("http client: {error}"))?;
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("download {url}: {error}"))?;
    response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("read {url}: {error}"))
}

fn cached_archive(manifest: &Manifest, path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    verify(manifest, &bytes).ok().map(|()| bytes)
}

fn extract(archive: &Path, destination: &Path, members: &[&str]) -> Result<(), String> {
    if destination.exists() {
        std::fs::remove_dir_all(destination)
            .map_err(|error| format!("clear extract dir: {error}"))?;
    }
    std::fs::create_dir_all(destination).map_err(|error| format!("create extract dir: {error}"))?;
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(destination)
        .args(members)
        .output()
        .map_err(|error| format!("run tar: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "tar failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Fetches (or reuses a verified cached copy of) the archive in `dir` and reads the collection.
///
/// # Errors
///
/// Returns a message when the download, verification, extraction or a read fails. A cached copy
/// that does not match the manifest is replaced; a fresh download that does not match is an error.
pub fn ensure_collection(manifest: &Manifest, dir: &Path) -> Result<Collection, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("create {}: {error}", dir.display()))?;
    let archive: PathBuf = dir.join("cran.tar.gz");
    if cached_archive(manifest, &archive).is_some() {
        println!("using verified cached archive {}", archive.display());
    } else {
        println!("downloading {}", manifest.source.url);
        let bytes = download(&manifest.source.url)?;
        verify(manifest, &bytes)?;
        std::fs::write(&archive, &bytes).map_err(|error| format!("write archive: {error}"))?;
    }
    let extracted = dir.join("extracted");
    let members = &manifest.source.members;
    extract(
        &archive,
        &extracted,
        &[&members.documents, &members.queries, &members.qrels],
    )?;
    let read = |name: &str| {
        std::fs::read_to_string(extracted.join(name))
            .map_err(|error| format!("read {name}: {error}"))
    };
    Ok(Collection {
        documents: read(&members.documents)?,
        queries: read(&members.queries)?,
        qrels: read(&members.qrels)?,
    })
}
