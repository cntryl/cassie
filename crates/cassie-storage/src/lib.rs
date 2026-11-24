//! Cassie's direct Midge storage adapter and persisted codecs.

mod error;

pub use error::StorageCodecError;

pub mod column_batch_format_v1;
pub mod key_codec;

mod scalar_index;
pub use scalar_index::{
    evaluate_expression_index_values, ScalarExpressionEvaluationError, ScalarExpressionEvaluator,
};

#[cfg(test)]
#[path = "scalar_index_tests.rs"]
mod scalar_index_tests;
