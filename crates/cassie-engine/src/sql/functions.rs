//! Engine adapters over the SQL crate's portable builtin declarations.

mod constant_folding;
mod datum_size;
pub(crate) mod hashing;
mod inputs;
mod numeric_identity;
mod percentile_identity;
pub(crate) mod primitive_casts;
mod table;
mod unsupported_server_control;
mod unsupported_server_files;

pub use cassie_sql::registry::functions::registry;
pub(crate) use cassie_sql::registry::functions::*;
pub(crate) use constant_folding::is_immutable_scalar;
pub(crate) use datum_size::{datum_size_kind, is_column_size, DatumSizeKind};
pub(crate) use hashing::{is_hash_function, return_type as hash_return_type};
pub(crate) use inputs::{
    aggregate_accepts_input, has_distinct_equality, input_signature, FunctionInputSignature,
};
pub(crate) use numeric_identity::has_unselected_numeric;
pub(crate) use percentile_identity::percentile_function_for_column;
pub(crate) use primitive_casts::{candidate as primitive_cast_candidate, PrimitiveCast};
pub(crate) use table::catalog_table_columns;
pub(crate) use unsupported_server_control::{
    is_server_control_helper, rejection as reject_server_control,
};
pub(crate) use unsupported_server_files::rejection as reject_server_file_access;

#[cfg(test)]
#[path = "functions/tests.rs"]
mod tests;
