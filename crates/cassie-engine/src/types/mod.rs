#[doc(hidden)]
pub mod array;
pub mod array_bounds;
pub mod array_value;
#[doc(hidden)]
pub use cassie_core::types::{boolean, bytea_input};
#[doc(hidden)]
pub mod builtin;
pub mod char_text;
pub mod composite;
pub mod domain;
pub mod enum_type;
pub mod extended;
#[doc(hidden)]
pub use cassie_core::types::{float_input, float_text};
pub mod graph;
#[doc(hidden)]
pub mod integer;
pub(crate) mod integer_bytea;
#[doc(hidden)]
pub mod json;
#[doc(hidden)]
pub mod jsonb;
pub(crate) mod lanes;
#[doc(hidden)]
pub use cassie_core::types::{name, uuid_text};
pub mod numeric;
pub use cassie_core::types::{object_id, row_identity, vector};
pub mod row;
pub mod schema;
pub(crate) mod semantic;
pub mod temporal;
pub mod typed;
pub mod value;

pub use array_value::{ArrayDimension, SqlArray};
pub use cassie_core::types::{
    BooleanInputError, ByteaInputError, FloatInputError, ObjectIdKind, ObjectIdentifierError,
    SizeInputError, Vector,
};
pub use composite::{CompositeField, CompositeRef, CompositeType, RecordValue};
pub use domain::{DomainCheck, DomainDefault, DomainRef, DomainType};
pub use enum_type::{EnumInputError, EnumMember, EnumRef, EnumType, EnumValue, EnumValuePlacement};
pub use integer::{IntegerOperation, IntegerWidth};
pub use integer_bytea::IntegerByteaError;
pub use row::Row;
pub use schema::{DataType, FieldSchema, Schema};
pub use value::Value;

pub(crate) mod semantic_index;

#[cfg(test)]
mod jsonb_numeric_cases;
