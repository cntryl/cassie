//! Allocation-free catalog-record identity, independent of container capacity.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::Hash;

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::types::DataType;

pub(super) trait ExactMetadataEq {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError>;
}

macro_rules! scalar_eq {
    ($($kind:ty),* $(,)?) => {$ (
        impl ExactMetadataEq for $kind {
            fn exact_eq(&self, other: &Self, controls: &QueryExecutionControls) -> Result<bool, CassieError> {
                controls.check_interruption()?;
                Ok(self == other)
            }
        }
    )*};
}

scalar_eq!(
    bool,
    u8,
    u16,
    u32,
    u64,
    i32,
    i64,
    usize,
    crate::catalog::DefaultSequenceOwnership,
    crate::types::ObjectIdKind,
    crate::types::extended::ExtendedType,
);

impl ExactMetadataEq for crate::catalog::ObjectCommentMeta {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        Ok(self
            .format_version
            .exact_eq(&other.format_version, controls)?
            && self
                .address
                .database
                .exact_eq(&other.address.database, controls)?
            && self
                .address
                .class_oid
                .exact_eq(&other.address.class_oid, controls)?
            && self
                .address
                .object_oid
                .exact_eq(&other.address.object_oid, controls)?
            && self
                .address
                .object_sub_id
                .exact_eq(&other.address.object_sub_id, controls)?
            && self.description.exact_eq(&other.description, controls)?)
    }
}

macro_rules! float_eq {
    ($($kind:ty),* $(,)?) => {$ (
        impl ExactMetadataEq for $kind {
            fn exact_eq(&self, other: &Self, controls: &QueryExecutionControls) -> Result<bool, CassieError> {
                controls.check_interruption()?;
                Ok(self.to_bits() == other.to_bits())
            }
        }
    )*};
}

float_eq!(f32, f64);

fn text_eq(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    controls.check_interruption()?;
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left
        .as_bytes()
        .chunks(4096)
        .zip(right.as_bytes().chunks(4096))
    {
        controls.check_interruption()?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

impl ExactMetadataEq for String {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        text_eq(self, other, controls)
    }
}

impl<T: ExactMetadataEq> ExactMetadataEq for Option<T> {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        match (self, other) {
            (Some(left), Some(right)) => left.exact_eq(right, controls),
            (None, None) => Ok(true),
            _ => Ok(false),
        }
    }
}

impl<T: ExactMetadataEq> ExactMetadataEq for Box<T> {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        self.as_ref().exact_eq(other.as_ref(), controls)
    }
}

impl<T: ExactMetadataEq> ExactMetadataEq for Vec<T> {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        if self.len() != other.len() {
            return Ok(false);
        }
        for (left, right) in self.iter().zip(other) {
            if !left.exact_eq(right, controls)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl<K: Eq + Hash + ExactMetadataEq, V: ExactMetadataEq> ExactMetadataEq for HashMap<K, V> {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        if self.len() != other.len() {
            return Ok(false);
        }
        for (key, value) in self {
            controls.check_interruption()?;
            let Some((other_key, other_value)) = other.get_key_value(key) else {
                return Ok(false);
            };
            if !key.exact_eq(other_key, controls)? || !value.exact_eq(other_value, controls)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl<K: Ord + ExactMetadataEq, V: ExactMetadataEq> ExactMetadataEq for BTreeMap<K, V> {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        if self.len() != other.len() {
            return Ok(false);
        }
        for (key, value) in self {
            controls.check_interruption()?;
            let Some((other_key, other_value)) = other.get_key_value(key) else {
                return Ok(false);
            };
            if !key.exact_eq(other_key, controls)? || !value.exact_eq(other_value, controls)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl<T: Ord + ExactMetadataEq> ExactMetadataEq for BTreeSet<T> {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        if self.len() != other.len() {
            return Ok(false);
        }
        for (left, right) in self.iter().zip(other) {
            if !left.exact_eq(right, controls)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl ExactMetadataEq for serde_json::Value {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        match (self, other) {
            (Self::Null, Self::Null) => Ok(true),
            (Self::Bool(left), Self::Bool(right)) => left.exact_eq(right, controls),
            (Self::Number(left), Self::Number(right)) => {
                text_eq(left.as_str(), right.as_str(), controls)
            }
            (Self::String(left), Self::String(right)) => left.exact_eq(right, controls),
            (Self::Array(left), Self::Array(right)) => left.exact_eq(right, controls),
            (Self::Object(left), Self::Object(right)) => {
                if left.len() != right.len() {
                    return Ok(false);
                }
                for (key, value) in left {
                    controls.check_interruption()?;
                    let Some(other_value) = right.get(key) else {
                        return Ok(false);
                    };
                    if !value.exact_eq(other_value, controls)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

impl ExactMetadataEq for DataType {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        match self {
            Self::Array(left) => match other {
                Self::Array(right) => left.exact_eq(right, controls),
                _ => Ok(false),
            },
            Self::Char { length: left } => match other {
                Self::Char { length: right } => left.exact_eq(right, controls),
                _ => Ok(false),
            },
            Self::Varchar { length: left } => match other {
                Self::Varchar { length: right } => left.exact_eq(right, controls),
                _ => Ok(false),
            },
            Self::Vector(left) => match other {
                Self::Vector(right) => left.exact_eq(right, controls),
                _ => Ok(false),
            },
            Self::ObjectId(left) => match other {
                Self::ObjectId(right) => left.exact_eq(right, controls),
                _ => Ok(false),
            },
            Self::Extended(left) => match other {
                Self::Extended(right) => left.exact_eq(right, controls),
                _ => Ok(false),
            },
            Self::Null
            | Self::SmallInt
            | Self::Int
            | Self::BigInt
            | Self::Float
            | Self::Boolean
            | Self::Name
            | Self::Text
            | Self::Uuid
            | Self::Bytea
            | Self::Date
            | Self::Time
            | Self::Timestamp
            | Self::Json
            | Self::Jsonb
            | Self::JsonPath => Ok(std::mem::discriminant(self) == std::mem::discriminant(other)),
        }
    }
}

impl ExactMetadataEq for crate::sql::ast::Expr {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        controls.check_interruption()?;
        Ok(crate::sql::check_comparison::exact_eq(self, other))
    }
}
