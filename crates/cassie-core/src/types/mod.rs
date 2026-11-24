pub mod array_bounds;
pub(crate) mod boolean;
pub(crate) mod bytea_input;
pub mod char_text;
pub mod enum_type;
pub(crate) mod float_input;
pub(crate) mod float_text;
pub(crate) mod integer_bytea;
pub(crate) mod name;
pub mod object_id;
pub mod row_identity;
mod size_input;
pub(crate) mod uuid_text;
pub mod vector;

pub use boolean::BooleanInputError;
pub use bytea_input::ByteaInputError;
pub use enum_type::{EnumInputError, EnumMember, EnumRef, EnumType, EnumValue, EnumValuePlacement};
pub use float_input::FloatInputError;
pub use integer_bytea::IntegerByteaError;
pub use object_id::{ObjectIdKind, ObjectIdentifierError};
pub use size_input::SizeInputError;
pub use vector::Vector;

#[cfg(test)]
mod enum_type_tests;
