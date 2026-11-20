//! Little-endian `f32` row files, id files and SHA-256 verification.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of the file at `path`, streamed.
///
/// # Errors
///
/// Returns a message when the file cannot be read.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Lowercase hex of `bytes`.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Fails with a message that points at the generation README when the file is
/// missing or its SHA-256 differs from `expected`.
///
/// # Errors
///
/// Returns the failure message.
pub fn verify_checksum(path: &Path, expected: &str, hint: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{} is missing; {hint}", path.display()));
    }
    let actual = sha256_file(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} has sha256 {actual} but the manifest records {expected}; {hint}",
            path.display()
        ))
    }
}

/// Decodes little-endian `f32` rows of `dimensions` values from `bytes`.
///
/// # Errors
///
/// Returns a message when `bytes` is not a whole number of rows.
pub fn decode_f32_rows(bytes: &[u8], dimensions: usize) -> Result<Vec<f32>, String> {
    let row_bytes = dimensions * 4;
    if dimensions == 0 || !bytes.len().is_multiple_of(row_bytes) {
        return Err(format!(
            "{} bytes is not a whole number of {dimensions}-dimension f32 rows",
            bytes.len()
        ));
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| f32::from_le_bytes(*word))
        .collect())
}

/// Reads a whole `f32` row file.
///
/// # Errors
///
/// Returns a message when the file cannot be read or is not whole rows.
pub fn read_f32_rows(path: &Path, dimensions: usize) -> Result<Vec<f32>, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    decode_f32_rows(&bytes, dimensions)
}

/// Reads one id per line.
///
/// # Errors
///
/// Returns a message when the file cannot be read.
pub fn read_lines(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(text.lines().map(str::to_string).collect())
}
