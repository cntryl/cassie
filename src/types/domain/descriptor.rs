//! Immutable versioned descriptors for domain types.
use std::ops::Deref;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::DataType;

/// The column name the keyword `VALUE` takes inside a domain CHECK expression.
pub const DOMAIN_VALUE_COLUMN: &str = "value";
/// Upper bound on CHECK constraints per domain, bounding per-write evaluation.
pub const MAX_DOMAIN_CHECKS: usize = 64;

/// A domain DEFAULT: its SQL text and the value or volatile expression it yields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainDefault {
    /// The expression as the user wrote it, for `pg_type.typdefault`.
    pub sql: String,
    /// The literal coerced to the base type; `None` for a volatile expression.
    pub value: Option<serde_json::Value>,
    /// The canonical volatile expression such as `now()`, when not a literal.
    pub expression: Option<String>,
}

/// One named CHECK constraint of a domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainCheck {
    pub name: String,
    /// The boolean expression text, with `VALUE` naming the checked value.
    pub sql: String,
    /// `false` for a constraint added `NOT VALID`; it is not enforced on
    /// existing rows but is enforced for new values.
    pub validated: bool,
}

/// A domain type descriptor.
///
/// `revision` increases whenever the constraint set, default or NOT NULL flag
/// changes. A descriptor is never mutated in place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainType {
    /// Allocated database-scoped type OID.
    pub oid: u32,
    /// Allocated OID of the matching array type (`_name`).
    pub array_oid: u32,
    /// Canonical `database.schema.name`.
    pub qualified_name: String,
    /// Schema-local type name.
    pub name: String,
    pub revision: u64,
    /// The base type; never itself a domain, composite or record.
    pub base: DataType,
    pub not_null: bool,
    pub default: Option<DomainDefault>,
    pub checks: Vec<DomainCheck>,
}

impl DomainType {
    /// The check called `name`, compared case-insensitively.
    #[must_use]
    pub fn check(&self, name: &str) -> Option<&DomainCheck> {
        self.checks
            .iter()
            .find(|check| check.name.eq_ignore_ascii_case(name))
    }
}

/// A shared, serializable handle to an immutable descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct DomainRef(Arc<DomainType>);

impl DomainRef {
    #[must_use]
    pub fn new(descriptor: DomainType) -> Self {
        Self(Arc::new(descriptor))
    }
}

impl Deref for DomainRef {
    type Target = DomainType;

    fn deref(&self) -> &DomainType {
        &self.0
    }
}

impl From<DomainType> for DomainRef {
    fn from(descriptor: DomainType) -> Self {
        Self::new(descriptor)
    }
}

impl Serialize for DomainRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DomainRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        DomainType::deserialize(deserializer).map(Self::new)
    }
}
