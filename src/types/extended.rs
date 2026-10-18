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

#[path = "extended/bit_ops.rs"]
pub mod bit_ops;
#[path = "extended/bit_string.rs"]
pub mod bit_string;
#[path = "extended/network.rs"]
pub mod network;
#[path = "extended/network_bit_casts.rs"]
mod network_bit_casts;
#[path = "extended/network_ops.rs"]
pub mod network_ops;
#[path = "extended/operations.rs"]
pub mod operations;
#[path = "extended/pg_lsn.rs"]
pub mod pg_lsn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtendedType {
    /// `real` / `float4`.
    Real,
    Inet,
    Cidr,
    Macaddr,
    Macaddr8,
    PgLsn,
    /// `bit(n)`; `None` is the unconstrained type of a bit-string literal.
    Bit {
        length: Option<u32>,
    },
    /// `bit varying(n)`; `None` has no maximum length.
    VarBit {
        length: Option<u32>,
    },
}

/// Operators whose meaning an extended family selects by operand types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedOperator {
    Add,
    Sub,
    And,
    Or,
    Xor,
    /// `<<`: shift for bit strings, "is contained by" for networks.
    ShiftLeft,
    /// `>>`: shift for bit strings, "contains" for networks.
    ShiftRight,
    Concat,
    ContainedOrEqual,
    ContainsOrEqual,
    Overlap,
}

/// Rejected text input for an extended type, with PostgreSQL's SQLSTATE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInputError {
    pub sqlstate: &'static str,
    pub message: String,
}

impl TypeInputError {
    #[must_use]
    pub const fn new(sqlstate: &'static str, message: String) -> Self {
        Self { sqlstate, message }
    }
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
    Network,
    MacAddress,
    BitString,
    Lsn,
}

impl ExtendedType {
    /// Every extended type, for name and OID lookups.
    pub const ALL: [Self; 8] = [
        Self::Real,
        Self::Inet,
        Self::Cidr,
        Self::Macaddr,
        Self::Macaddr8,
        Self::PgLsn,
        Self::Bit { length: None },
        Self::VarBit { length: None },
    ];

    /// The SQL spelling Cassie prints in `type_name` and catalog columns.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Inet => "inet",
            Self::Cidr => "cidr",
            Self::Macaddr => "macaddr",
            Self::Macaddr8 => "macaddr8",
            Self::PgLsn => "pg_lsn",
            Self::Bit { .. } => "bit",
            Self::VarBit { .. } => "bit varying",
        }
    }

    /// The `pg_type.typname`.
    #[must_use]
    pub const fn typname(self) -> &'static str {
        match self {
            Self::Real => "float4",
            Self::Inet => "inet",
            Self::Cidr => "cidr",
            Self::Macaddr => "macaddr",
            Self::Macaddr8 => "macaddr8",
            Self::PgLsn => "pg_lsn",
            Self::Bit { .. } => "bit",
            Self::VarBit { .. } => "varbit",
        }
    }

    /// The PostgreSQL 18 type OID.
    #[must_use]
    pub const fn type_oid(self) -> u32 {
        match self {
            Self::Real => 700,
            Self::Inet => 869,
            Self::Cidr => 650,
            Self::Macaddr => 829,
            Self::Macaddr8 => 774,
            Self::PgLsn => 3220,
            Self::Bit { .. } => 1560,
            Self::VarBit { .. } => 1562,
        }
    }

    #[must_use]
    pub const fn typlen(self) -> i16 {
        match self {
            Self::Real => 4,
            Self::Macaddr => 6,
            Self::Macaddr8 | Self::PgLsn => 8,
            Self::Inet | Self::Cidr | Self::Bit { .. } | Self::VarBit { .. } => -1,
        }
    }

    /// The `pg_type.typcategory`.
    #[must_use]
    pub const fn category(self) -> char {
        match self {
            Self::Real => 'N',
            Self::Inet | Self::Cidr => 'I',
            Self::Macaddr | Self::Macaddr8 | Self::PgLsn => 'U',
            Self::Bit { .. } | Self::VarBit { .. } => 'V',
        }
    }

    #[must_use]
    pub const fn family(self) -> ExtendedFamily {
        match self {
            Self::Real => ExtendedFamily::Numeric,
            Self::Inet | Self::Cidr => ExtendedFamily::Network,
            Self::Macaddr | Self::Macaddr8 => ExtendedFamily::MacAddress,
            Self::Bit { .. } | Self::VarBit { .. } => ExtendedFamily::BitString,
            Self::PgLsn => ExtendedFamily::Lsn,
        }
    }

    /// Whether a parameter declared with this type's OID binds as this type.
    /// A family whose operators and aggregates are not complete keeps its
    /// parameters on the untyped numeric adapter path until they are.
    #[must_use]
    pub const fn binds_parameters(self) -> bool {
        match self {
            Self::Real => false,
            Self::Inet
            | Self::Cidr
            | Self::Macaddr
            | Self::Macaddr8
            | Self::PgLsn
            | Self::Bit { .. }
            | Self::VarBit { .. } => true,
        }
    }

    /// The type without its length modifier, as registered in the catalog.
    #[must_use]
    pub const fn base(self) -> Self {
        match self {
            Self::Bit { .. } => Self::Bit { length: None },
            Self::VarBit { .. } => Self::VarBit { length: None },
            other => other,
        }
    }

    /// The `atttypmod` of the type: a bit-string length, otherwise -1.
    #[must_use]
    pub fn typmod(self) -> i32 {
        match self {
            Self::Bit {
                length: Some(length),
            }
            | Self::VarBit {
                length: Some(length),
            } => i32::try_from(length).unwrap_or(-1),
            _ => -1,
        }
    }

    /// The SQL spelling including any length modifier.
    #[must_use]
    pub fn type_name(self) -> String {
        match self {
            Self::Bit {
                length: Some(length),
            } => format!("bit({length})"),
            Self::VarBit {
                length: Some(length),
            } => format!("bit varying({length})"),
            other => other.name().to_string(),
        }
    }

    /// Resolves a lower-cased SQL type name (without typmod) to a type.
    #[must_use]
    pub fn from_sql_name(name: &str) -> Option<Self> {
        match name {
            "real" | "float4" => Some(Self::Real),
            "inet" => Some(Self::Inet),
            "cidr" => Some(Self::Cidr),
            "macaddr" => Some(Self::Macaddr),
            "macaddr8" => Some(Self::Macaddr8),
            "pg_lsn" => Some(Self::PgLsn),
            "bit" => Some(Self::Bit { length: Some(1) }),
            "varbit" | "bit varying" => Some(Self::VarBit { length: None }),
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
            Self::Inet
            | Self::Cidr
            | Self::Macaddr
            | Self::Macaddr8
            | Self::PgLsn
            | Self::Bit { .. }
            | Self::VarBit { .. } => network_bit_casts::parse_text(self, text),
        }
    }

    /// Converts an integer, float or text scalar into this type (`CAST`).
    ///
    /// # Errors
    /// Returns PostgreSQL's input or range error.
    pub fn cast_from(self, value: &super::Value) -> Result<TypedValue, TypeInputError> {
        match (self, value) {
            (_, super::Value::Typed(typed)) if typed.kind() == self => Ok(typed.clone()),
            (
                Self::Inet
                | Self::Cidr
                | Self::Macaddr
                | Self::Macaddr8
                | Self::PgLsn
                | Self::Bit { .. }
                | Self::VarBit { .. },
                value,
            ) => network_bit_casts::cast_from(self, value),
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
            Self::Inet
            | Self::Cidr
            | Self::Macaddr
            | Self::Macaddr8
            | Self::PgLsn
            | Self::Bit { .. }
            | Self::VarBit { .. } => network_bit_casts::receive_binary(self, bytes),
        }
    }
}
