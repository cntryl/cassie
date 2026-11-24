//! Cassie's direct Midge storage adapter and persisted codecs.

mod error;

pub use error::StorageCodecError;

pub mod adapter;
pub mod cardinality;
pub mod native;
pub mod operation_stats;
pub mod runtime_feedback;
pub mod column_batch_format_v1;
pub mod key_codec;
pub mod row_blob;
pub use adapter::error::StorageError;
mod catalog;
pub(crate) use cassie_vector::{self as vector, types as embeddings};

mod scalar_index;
pub use scalar_index::{
    evaluate_expression_index_values, ScalarExpressionEvaluationError, ScalarExpressionEvaluator,
};

#[cfg(test)]
#[path = "scalar_index_tests.rs"]
mod scalar_index_tests;
