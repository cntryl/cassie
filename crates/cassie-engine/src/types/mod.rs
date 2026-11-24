//! Engine-side adapters and compatibility exports for shared value types.

pub use cassie_core::types::{
    array_bounds, array_value, boolean, builtin, bytea_input, char_text, composite, domain,
    enum_type, float_input, float_text, integer, integer_bytea, json, jsonb_format, name, numeric,
    object_id, row, row_identity, schema, temporal, typed, uuid_text, value, vector,
    ArrayDimension, BooleanInputError, ByteaInputError, CompositeField, CompositeRef,
    CompositeType, DataType, DomainCheck, DomainDefault, DomainRef, DomainType, EnumInputError,
    EnumMember, EnumRef, EnumType, EnumValue, EnumValuePlacement, FieldSchema, FloatInputError,
    IntegerArithmeticError, IntegerByteaError, IntegerInputError, IntegerOperation, IntegerWidth,
    ObjectIdKind, ObjectIdentifierError, RecordValue, Row, Schema, SizeInputError, SqlArray,
    TypedKey, TypedValue, Value, Vector,
};

#[doc(hidden)]
pub mod array;
#[doc(hidden)]
pub mod context;
pub mod extended;
pub mod graph;
#[doc(hidden)]
pub mod jsonb;
pub(crate) mod lanes;
pub(crate) mod numeric_validation;
pub(crate) mod semantic;
pub(crate) mod semantic_index;
#[doc(hidden)]
pub use context::parameter_from_text_with_controls as temporal_parameter_from_text_with_controls;

#[path = "primitive_copy.rs"]
pub(crate) mod primitive_copy;
#[cfg(test)]
#[path = "array_value/primitive_copy_tests.rs"]
mod primitive_copy_tests;
