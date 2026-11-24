//! Cassie's direct Midge storage adapter and persisted codecs.

mod error;

pub use error::StorageCodecError;

pub mod column_batch_format_v1;
pub mod key_codec;
