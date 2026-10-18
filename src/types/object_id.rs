//! PostgreSQL object identifier types: `oid` and its `reg*` aliases.
//!
//! Every kind is an unsigned 32-bit catalog identity carried as
//! `Value::Int64`. The kind only selects text input/output, which resolves
//! catalog names; comparison, hashing and binary framing use the integer.
use serde::{Deserialize, Serialize};

/// Largest value an `oid` can hold.
pub const MAX_OID: i64 = 4_294_967_295;

/// First OID PostgreSQL assigns to user-created objects.
pub const FIRST_NORMAL_OBJECT_ID: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectIdKind {
    Oid,
    RegProc,
    RegProcedure,
    RegOper,
    RegOperator,
    RegClass,
    RegType,
    RegConfig,
    RegDictionary,
    RegNamespace,
    RegRole,
    RegCollation,
}

impl ObjectIdKind {
    pub const ALL: [Self; 12] = [
        Self::Oid,
        Self::RegProc,
        Self::RegProcedure,
        Self::RegOper,
        Self::RegOperator,
        Self::RegClass,
        Self::RegType,
        Self::RegConfig,
        Self::RegDictionary,
        Self::RegNamespace,
        Self::RegRole,
        Self::RegCollation,
    ];

    /// The `pg_type.typname` spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Oid => "oid",
            Self::RegProc => "regproc",
            Self::RegProcedure => "regprocedure",
            Self::RegOper => "regoper",
            Self::RegOperator => "regoperator",
            Self::RegClass => "regclass",
            Self::RegType => "regtype",
            Self::RegConfig => "regconfig",
            Self::RegDictionary => "regdictionary",
            Self::RegNamespace => "regnamespace",
            Self::RegRole => "regrole",
            Self::RegCollation => "regcollation",
        }
    }

    /// The PostgreSQL 18 type OID.
    #[must_use]
    pub const fn type_oid(self) -> u32 {
        match self {
            Self::Oid => 26,
            Self::RegProc => 24,
            Self::RegProcedure => 2202,
            Self::RegOper => 2203,
            Self::RegOperator => 2204,
            Self::RegClass => 2205,
            Self::RegType => 2206,
            Self::RegConfig => 3734,
            Self::RegDictionary => 3769,
            Self::RegNamespace => 4089,
            Self::RegRole => 4096,
            Self::RegCollation => 4191,
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name().eq_ignore_ascii_case(name))
    }

    /// Whether text input/output goes through a catalog name lookup.
    #[must_use]
    pub const fn is_alias(self) -> bool {
        !matches!(self, Self::Oid)
    }
}

/// Parses `oidin` input: a decimal number in `0..=4294967295`. PostgreSQL
/// also accepts negative values down to `-2147483648`, wrapping them.
///
/// # Errors
/// Returns PostgreSQL's invalid-syntax or out-of-range error.
pub fn parse_oid_text(text: &str) -> Result<u32, ObjectIdentifierError> {
    let value = text
        .trim()
        .parse::<i64>()
        .map_err(|_| ObjectIdentifierError::Syntax(text.to_string()))?;
    oid_from_integer(value).map_err(|_| ObjectIdentifierError::OutOfRange(text.to_string()))
}

/// Converts an integer to an OID with PostgreSQL's range rules.
///
/// # Errors
/// Returns the rejected value when it is outside `-2^31..=2^32-1`.
pub fn oid_from_integer(value: i64) -> Result<u32, i64> {
    if (0..=MAX_OID).contains(&value) {
        return u32::try_from(value).map_err(|_| value);
    }
    if (i64::from(i32::MIN)..0).contains(&value) {
        let narrowed = i32::try_from(value).map_err(|_| value)?;
        return Ok(narrowed.cast_unsigned());
    }
    Err(value)
}

/// Input and lookup failures for `oid` and the `reg*` types, with
/// PostgreSQL's SQLSTATE and message for each case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectIdentifierError {
    /// `oidin` rejected non-numeric input.
    Syntax(String),
    /// The number does not fit an `oid`.
    OutOfRange(String),
    /// The name is not a valid (possibly qualified) identifier list.
    InvalidName(String),
    /// A `regprocedure`/`regoperator` signature is malformed.
    Signature(String),
    /// The named object does not exist.
    Undefined { kind: ObjectIdKind, name: String },
    /// A bare `regproc`/`regoper` name matches several overloads.
    Ambiguous { kind: ObjectIdKind, name: String },
}

impl ObjectIdentifierError {
    #[must_use]
    pub fn sqlstate(&self) -> &'static str {
        match self {
            Self::Syntax(_) | Self::Signature(_) => "22P02",
            Self::OutOfRange(_) => "22003",
            Self::InvalidName(_) => "42602",
            Self::Ambiguous { .. } => "42725",
            Self::Undefined { kind, .. } => match kind {
                ObjectIdKind::RegClass => "42P01",
                ObjectIdKind::RegProc
                | ObjectIdKind::RegProcedure
                | ObjectIdKind::RegOper
                | ObjectIdKind::RegOperator => "42883",
                ObjectIdKind::RegNamespace => "3F000",
                ObjectIdKind::Oid
                | ObjectIdKind::RegType
                | ObjectIdKind::RegConfig
                | ObjectIdKind::RegDictionary
                | ObjectIdKind::RegRole
                | ObjectIdKind::RegCollation => "42704",
            },
        }
    }
}

impl std::fmt::Display for ObjectIdentifierError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(text) => {
                write!(formatter, "invalid input syntax for type oid: \"{text}\"")
            }
            Self::OutOfRange(text) => {
                write!(formatter, "value \"{text}\" is out of range for type oid")
            }
            Self::InvalidName(_) => formatter.write_str("invalid name syntax"),
            Self::Signature(message) => formatter.write_str(message),
            Self::Undefined { kind, name } => {
                let object = match kind {
                    ObjectIdKind::RegClass => "relation",
                    ObjectIdKind::RegProc | ObjectIdKind::RegProcedure => "function",
                    ObjectIdKind::RegOper | ObjectIdKind::RegOperator => {
                        return write!(formatter, "operator does not exist: {name}");
                    }
                    ObjectIdKind::RegType | ObjectIdKind::Oid => "type",
                    ObjectIdKind::RegNamespace => "schema",
                    ObjectIdKind::RegRole => "role",
                    ObjectIdKind::RegCollation => "collation",
                    ObjectIdKind::RegConfig => "text search configuration",
                    ObjectIdKind::RegDictionary => "text search dictionary",
                };
                if matches!(kind, ObjectIdKind::RegProcedure) {
                    write!(formatter, "{object} {name} does not exist")
                } else {
                    write!(formatter, "{object} \"{name}\" does not exist")
                }
            }
            Self::Ambiguous { kind, name } => {
                let object = if matches!(kind, ObjectIdKind::RegOper) {
                    "operator"
                } else {
                    "function"
                };
                write!(formatter, "more than one {object} named \"{name}\"")
            }
        }
    }
}

impl std::error::Error for ObjectIdentifierError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_accept_the_unsigned_oid_boundary() {
        // Arrange
        let text = "4294967295";

        // Act
        let parsed = parse_oid_text(text);

        // Assert
        assert_eq!(parsed, Ok(u32::MAX));
    }

    #[test]
    fn should_reject_an_oid_past_the_unsigned_boundary() {
        // Arrange
        let text = "4294967296";

        // Act
        let parsed = parse_oid_text(text);

        // Assert
        assert_eq!(
            parsed,
            Err(ObjectIdentifierError::OutOfRange(text.to_string()))
        );
    }

    #[test]
    fn should_wrap_negative_int4_input_like_postgresql() {
        // Arrange
        let value = -1;

        // Act
        let oid = oid_from_integer(value);

        // Assert
        assert_eq!(oid, Ok(u32::MAX));
    }
}
