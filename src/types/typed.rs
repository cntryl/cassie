//! Native runtime carriers for [`ExtendedType`] values.
//!
//! Inside the engine a value keeps its native form, so comparison, grouping
//! and indexing follow PostgreSQL. At the result boundary it renders as its
//! PostgreSQL text output, matching the text wire format.
use serde::{Deserialize, Serialize};

use super::extended::bit_string::{self, BitString};
use super::extended::network::{self, NetAddr, NetworkKey};
use super::extended::{numeric, pg_lsn, real, ExtendedType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedValue {
    Real(f32),
    Numeric(numeric::Numeric),
    Inet(NetAddr),
    Cidr(NetAddr),
    Macaddr([u8; 6]),
    Macaddr8([u8; 8]),
    PgLsn(u64),
    Bit(BitString),
    VarBit(BitString),
}

/// A totally ordered, hashable key with PostgreSQL ordering semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypedKey {
    /// `float4` with PostgreSQL's total order (NaN sorts last, -0 = 0).
    Real(u32),
    /// `numeric` with PostgreSQL order (NaN last, `1.0` = `1.00`).
    Numeric(numeric::NumericKey),
    /// `inet` and `cidr` share PostgreSQL's `network_cmp` order.
    Network(NetworkKey),
    Macaddr([u8; 6]),
    Macaddr8([u8; 8]),
    PgLsn(u64),
    /// `bit` and `bit varying` share `bit_cmp`: bytes, then bit length.
    Bits(Vec<u8>, usize),
}

impl TypedValue {
    #[must_use]
    pub const fn kind(&self) -> ExtendedType {
        match self {
            Self::Real(_) => ExtendedType::Real,
            Self::Numeric(_) => ExtendedType::Numeric(None),
            Self::Inet(_) => ExtendedType::Inet,
            Self::Cidr(_) => ExtendedType::Cidr,
            Self::Macaddr(_) => ExtendedType::Macaddr,
            Self::Macaddr8(_) => ExtendedType::Macaddr8,
            Self::PgLsn(_) => ExtendedType::PgLsn,
            Self::Bit(_) => ExtendedType::Bit { length: None },
            Self::VarBit(_) => ExtendedType::VarBit { length: None },
        }
    }

    /// `typoutput`: the canonical PostgreSQL text form.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Real(value) => real::format_text(*value),
            Self::Numeric(value) => value.to_string(),
            Self::Inet(value) => network::format_network(*value, false),
            Self::Cidr(value) => network::format_network(*value, true),
            Self::Macaddr(value) => network::format_mac(value),
            Self::Macaddr8(value) => network::format_mac(value),
            Self::PgLsn(value) => pg_lsn::format_text(*value),
            Self::Bit(value) | Self::VarBit(value) => bit_string::format_text(value),
        }
    }

    /// The JSON carrier stored in documents; storage re-encodes it natively.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.text())
    }

    #[must_use]
    pub fn key(&self) -> TypedKey {
        match self {
            Self::Real(value) => TypedKey::Real(real::order_key(*value)),
            Self::Numeric(value) => TypedKey::Numeric(value.order_key()),
            Self::Inet(value) | Self::Cidr(value) => TypedKey::Network(value.order_key()),
            Self::Macaddr(value) => TypedKey::Macaddr(*value),
            Self::Macaddr8(value) => TypedKey::Macaddr8(*value),
            Self::PgLsn(value) => TypedKey::PgLsn(*value),
            Self::Bit(value) | Self::VarBit(value) => {
                let (bytes, len) = value.order_key();
                TypedKey::Bits(bytes, len)
            }
        }
    }

    /// Heap bytes a clone retains, for query memory accounting.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        match self {
            Self::Bit(value) | Self::VarBit(value) => value.heap_bytes(),
            Self::Numeric(numeric::Numeric::Finite { mantissa, .. }) => {
                usize::try_from(mantissa.bits().div_ceil(8)).unwrap_or(usize::MAX)
            }
            Self::Real(_)
            | Self::Numeric(_)
            | Self::Inet(_)
            | Self::Cidr(_)
            | Self::Macaddr(_)
            | Self::Macaddr8(_)
            | Self::PgLsn(_) => 0,
        }
    }

    /// `typsend`: PostgreSQL binary output.
    #[must_use]
    pub fn send_binary(&self) -> Vec<u8> {
        match self {
            Self::Real(value) => value.to_be_bytes().to_vec(),
            Self::Numeric(value) => value.send_binary(),
            Self::Inet(value) => network::send_network(*value, false),
            Self::Cidr(value) => network::send_network(*value, true),
            Self::Macaddr(value) => value.to_vec(),
            Self::Macaddr8(value) => value.to_vec(),
            Self::PgLsn(value) => value.to_be_bytes().to_vec(),
            Self::Bit(value) | Self::VarBit(value) => bit_string::send_binary(value),
        }
    }

    /// The value as `f64` when the family is numeric.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Real(value) => Some(f64::from(*value)),
            Self::Numeric(value) => Some(value.to_f64()),
            Self::Inet(_)
            | Self::Cidr(_)
            | Self::Macaddr(_)
            | Self::Macaddr8(_)
            | Self::PgLsn(_)
            | Self::Bit(_)
            | Self::VarBit(_) => None,
        }
    }
}
