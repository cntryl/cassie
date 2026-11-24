//! Column `STORAGE` and `COMPRESSION` metadata (#826).
//!
//! These settings are catalog metadata only. Cassie stores rows through
//! Midge, so neither setting changes physical layout, TOAST-like behavior or
//! value encoding. They exist so PostgreSQL DDL that declares them is
//! accepted, reported through `pg_attribute.attstorage` and
//! `pg_attribute.attcompression`, and copied by `LIKE ... INCLUDING STORAGE`
//! and `INCLUDING COMPRESSION`.
use crate::types::{extended::ExtendedType, DataType};
use serde::{Deserialize, Serialize};

/// An explicit `STORAGE` strategy chosen for a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnStorage {
    Plain,
    External,
    Extended,
    Main,
}

impl ColumnStorage {
    /// Parses a `STORAGE` keyword; `DEFAULT` yields `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns the rejected keyword when it is not a PostgreSQL strategy.
    pub fn parse(keyword: &str) -> Result<Option<Self>, String> {
        match keyword.to_ascii_lowercase().as_str() {
            "plain" => Ok(Some(Self::Plain)),
            "external" => Ok(Some(Self::External)),
            "extended" => Ok(Some(Self::Extended)),
            "main" => Ok(Some(Self::Main)),
            "default" => Ok(None),
            _ => Err(format!("invalid storage type \"{keyword}\"")),
        }
    }

    /// `pg_attribute.attstorage` code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Plain => "p",
            Self::External => "e",
            Self::Extended => "x",
            Self::Main => "m",
        }
    }
}

/// An explicit `COMPRESSION` method chosen for a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnCompression {
    Pglz,
    Lz4,
}

impl ColumnCompression {
    /// Parses a `COMPRESSION` method; `default` yields `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns the rejected method when it is not `pglz`, `lz4` or `default`.
    pub fn parse(method: &str) -> Result<Option<Self>, String> {
        match method.to_ascii_lowercase().as_str() {
            "pglz" => Ok(Some(Self::Pglz)),
            "lz4" => Ok(Some(Self::Lz4)),
            "default" => Ok(None),
            _ => Err(format!("invalid compression method \"{method}\"")),
        }
    }

    /// `pg_attribute.attcompression` code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Pglz => "p",
            Self::Lz4 => "l",
        }
    }
}

/// One `ALTER COLUMN SET STORAGE` or `SET COMPRESSION` change; `None` resets
/// the setting to the type default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnStorageSetting {
    Storage(Option<ColumnStorage>),
    Compression(Option<ColumnCompression>),
}

/// The storage strategy PostgreSQL 18 assigns to a type by default.
///
/// Fixed-width types are `PLAIN`, `numeric` and `inet`/`cidr` are `MAIN`, and
/// other variable-length types are `EXTENDED`. Domains are reported as
/// `EXTENDED` because their base type is not resolved here.
#[must_use]
pub const fn default_storage(data_type: &DataType) -> ColumnStorage {
    match data_type {
        DataType::Null
        | DataType::SmallInt
        | DataType::Int
        | DataType::BigInt
        | DataType::Float
        | DataType::Boolean
        | DataType::Name
        | DataType::Uuid
        | DataType::Date
        | DataType::Time
        | DataType::Timestamp
        | DataType::Enum(_)
        | DataType::ObjectId(_) => ColumnStorage::Plain,
        DataType::Extended(extended) => extended_default_storage(*extended),
        DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::Bytea
        | DataType::Vector(_)
        | DataType::Json
        | DataType::Jsonb
        | DataType::JsonPath
        | DataType::Array(_)
        | DataType::Composite(_)
        | DataType::Domain(_) => ColumnStorage::Extended,
    }
}

const fn extended_default_storage(extended: ExtendedType) -> ColumnStorage {
    match extended {
        ExtendedType::Real
        | ExtendedType::Macaddr
        | ExtendedType::Macaddr8
        | ExtendedType::PgLsn
        | ExtendedType::Date
        | ExtendedType::Time { .. }
        | ExtendedType::TimeTz { .. }
        | ExtendedType::Timestamp { .. }
        | ExtendedType::TimestampTz { .. }
        | ExtendedType::Interval { .. } => ColumnStorage::Plain,
        ExtendedType::Numeric(_) | ExtendedType::Inet | ExtendedType::Cidr => ColumnStorage::Main,
        ExtendedType::Bit { .. } | ExtendedType::VarBit { .. } => ColumnStorage::Extended,
    }
}

/// Validates explicit settings against the column type, like PostgreSQL.
///
/// A type whose default storage is `PLAIN` is not variable-length, so it can
/// only be `PLAIN` and cannot be compressed.
///
/// # Errors
///
/// Returns PostgreSQL's message for the rejected combination.
/// Rejects storage or compression a fixed-width type cannot take. PostgreSQL
/// reports these with SQLSTATE 42804 (`datatype_mismatch`).
pub fn validate_column_storage(
    data_type: &DataType,
    storage: Option<ColumnStorage>,
    compression: Option<ColumnCompression>,
) -> Result<(), String> {
    let fixed_width = default_storage(data_type) == ColumnStorage::Plain;
    if fixed_width && storage.is_some_and(|storage| storage != ColumnStorage::Plain) {
        return Err(format!(
            "column data type {} can only have storage PLAIN",
            data_type.type_name()
        ));
    }
    if fixed_width && compression.is_some() {
        return Err(format!(
            "column data type {} does not support compression",
            data_type.type_name()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_postgres_default_storage_by_type() {
        // Arrange
        let numeric = DataType::Extended(ExtendedType::Numeric(None));

        // Act
        let (int, text, number) = (
            default_storage(&DataType::Int),
            default_storage(&DataType::Text),
            default_storage(&numeric),
        );

        // Assert
        assert_eq!(int.code(), "p");
        assert_eq!(text.code(), "x");
        assert_eq!(number.code(), "m");
    }

    #[test]
    fn should_reject_storage_overrides_on_fixed_width_types() {
        // Arrange
        let plain = Some(ColumnStorage::Plain);

        // Act
        let storage = validate_column_storage(&DataType::Int, Some(ColumnStorage::External), None);
        let compression =
            validate_column_storage(&DataType::Int, None, Some(ColumnCompression::Lz4));
        let accepted = validate_column_storage(&DataType::Int, plain, None);

        // Assert
        assert!(storage.is_err_and(|message| message.contains("can only have storage PLAIN")));
        assert!(compression.is_err_and(|message| message.contains("does not support compression")));
        assert!(accepted.is_ok());
    }

    #[test]
    fn should_parse_default_keywords_as_unset() {
        // Arrange
        let keywords = ["DEFAULT", "default"];

        // Act
        let parsed = keywords.map(|keyword| {
            (
                ColumnStorage::parse(keyword),
                ColumnCompression::parse(keyword),
            )
        });

        // Assert
        assert!(parsed
            .iter()
            .all(|(storage, compression)| storage == &Ok(None) && compression == &Ok(None)));
        assert!(ColumnStorage::parse("bogus").is_err());
        assert!(ColumnCompression::parse("bogus").is_err());
    }
}
