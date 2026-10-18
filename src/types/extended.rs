//! Natively represented PostgreSQL types beyond the original scalar set.
//!
//! Each family owns a module (`real`, ...) that implements text input and
//! output, ordering, storage and wire framing for its [`TypedValue`]
//! carrier. Every shared `match` over [`DataType`](super::DataType) or
//! [`Value`](super::Value) delegates here through one arm, so a new family
//! touches only its own module and the dispatch below.
use serde::{Deserialize, Serialize};

use super::typed::TypedValue;

#[path = "extended/real.rs"]
pub mod real;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtendedType {
    /// `real` / `float4`.
    Real,
}

/// Rejected text input for an extended type, with PostgreSQL's SQLSTATE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInputError {
    pub sqlstate: &'static str,
    pub message: String,
}

impl std::fmt::Display for TypeInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TypeInputError {}

/// The comparison family a type's operands belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtendedFamily {
    Numeric,
}

impl ExtendedType {
    /// Every extended type, for name and OID lookups.
    pub const ALL: [Self; 1] = [Self::Real];

    /// The SQL spelling Cassie prints in `type_name` and catalog columns.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Real => "real",
        }
    }

    /// The `pg_type.typname`.
    #[must_use]
    pub const fn typname(self) -> &'static str {
        match self {
            Self::Real => "float4",
        }
    }

    /// The PostgreSQL 18 type OID.
    #[must_use]
    pub const fn type_oid(self) -> u32 {
        match self {
            Self::Real => 700,
        }
    }

    #[must_use]
    pub const fn typlen(self) -> i16 {
        match self {
            Self::Real => 4,
        }
    }

    /// The `pg_type.typcategory`.
    #[must_use]
    pub const fn category(self) -> char {
        match self {
            Self::Real => 'N',
        }
    }

    #[must_use]
    pub const fn family(self) -> ExtendedFamily {
        match self {
            Self::Real => ExtendedFamily::Numeric,
        }
    }

    /// Resolves a lower-cased SQL type name (without typmod) to a type.
    #[must_use]
    pub fn from_sql_name(name: &str) -> Option<Self> {
        match name {
            "real" | "float4" => Some(Self::Real),
            _ => None,
        }
    }

    #[must_use]
    pub fn for_oid(oid: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.type_oid() == oid)
    }

    /// `typinput`: parses PostgreSQL text input.
    ///
    /// # Errors
    /// Returns PostgreSQL's input error for malformed or out-of-range text.
    pub fn parse_text(self, text: &str) -> Result<TypedValue, TypeInputError> {
        match self {
            Self::Real => real::parse_text(text).map(TypedValue::Real),
        }
    }

    /// Converts an integer, float or text scalar into this type (`CAST`).
    ///
    /// # Errors
    /// Returns PostgreSQL's input or range error.
    pub fn cast_from(self, value: &super::Value) -> Result<TypedValue, TypeInputError> {
        match (self, value) {
            (_, super::Value::Typed(typed)) if typed.kind() == self => Ok(typed.clone()),
            (Self::Real, super::Value::Int64(integer)) => real::from_f64(
                crate::types::numeric::i64_to_f64(*integer),
                &integer.to_string(),
            )
            .map(TypedValue::Real),
            (Self::Real, super::Value::Float64(float)) => {
                real::from_f64(*float, &float.to_string()).map(TypedValue::Real)
            }
            (_, super::Value::Typed(typed)) => self.parse_text(&typed.text()),
            (_, super::Value::String(text)) => self.parse_text(text),
            _ => Err(TypeInputError {
                sqlstate: "42846",
                message: format!("cannot cast value to {}", self.name()),
            }),
        }
    }

    /// Reads the JSON storage carrier written by [`TypedValue::to_json`].
    ///
    /// # Errors
    /// Returns an input error when the stored carrier is not valid text input.
    pub fn from_json(self, value: &serde_json::Value) -> Result<TypedValue, TypeInputError> {
        match value {
            serde_json::Value::String(text) => self.parse_text(text),
            serde_json::Value::Number(number) => self.parse_text(&number.to_string()),
            _ => Err(TypeInputError {
                sqlstate: "22P02",
                message: format!("invalid stored value for type {}", self.name()),
            }),
        }
    }

    /// `typreceive`: decodes PostgreSQL binary input.
    ///
    /// # Errors
    /// Returns an error message for malformed frames.
    pub fn receive_binary(self, bytes: &[u8]) -> Result<TypedValue, String> {
        match self {
            Self::Real => real::receive_binary(bytes).map(TypedValue::Real),
        }
    }
}
