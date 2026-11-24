//! Portable, storage-owned errors returned across the engine boundary.

/// A persisted column codec rejected malformed or unsupported bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StorageCodecError {
    /// The encoded bytes cannot be interpreted using the persisted codec.
    #[error("{0}")]
    InvalidEncoding(String),
    /// A scalar index value cannot be represented by the persisted key format.
    #[error("{0}")]
    UnsupportedValue(String),
    /// A row payload is not an object accepted by the storage format.
    #[error("{0}")]
    InvalidValue(String),
}
