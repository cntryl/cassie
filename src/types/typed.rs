//! Native runtime carriers for [`ExtendedType`] values.
//!
//! Inside the engine a value keeps its native form, so comparison, grouping
//! and indexing follow PostgreSQL. At the result boundary it renders as its
//! PostgreSQL text output, matching the text wire format.
use serde::{Deserialize, Serialize};

use super::extended::{real, ExtendedType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedValue {
    Real(f32),
}

/// A totally ordered, hashable key with PostgreSQL ordering semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypedKey {
    /// `float4` with PostgreSQL's total order (NaN sorts last, -0 = 0).
    Real(u32),
}

impl TypedValue {
    #[must_use]
    pub const fn kind(&self) -> ExtendedType {
        match self {
            Self::Real(_) => ExtendedType::Real,
        }
    }

    /// `typoutput`: the canonical PostgreSQL text form.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Real(value) => real::format_text(*value),
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
        }
    }

    /// Heap bytes a clone retains, for query memory accounting.
    #[must_use]
    pub const fn heap_bytes(&self) -> usize {
        match self {
            Self::Real(_) => 0,
        }
    }

    /// `typsend`: PostgreSQL binary output.
    #[must_use]
    pub fn send_binary(&self) -> Vec<u8> {
        match self {
            Self::Real(value) => value.to_be_bytes().to_vec(),
        }
    }

    /// The value as `f64` when the family is numeric.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Real(value) => Some(f64::from(*value)),
        }
    }
}
