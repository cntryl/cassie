use serde::{Deserialize, Serialize};

use super::builtin::{type_metadata, TypeMetadata};

/// First type OID of the `VECTOR(n)` family: `VECTOR(n)` is this plus `n`.
/// The range sits above the array OIDs (`34000..44000`) so no vector width
/// can resolve to an array type.
pub const VECTOR_OID_BASE: i64 = 100_000;
/// Widest vector the OID range covers; the binary vector format stores the
/// dimension count as an `int16`.
pub const VECTOR_OID_MAX_DIMENSIONS: usize = 32_767;
/// Array element OIDs for vectors keep the historical `33000 + n` index so
/// arrays of vectors do not move onto a scalar array's OID.
const LEGACY_VECTOR_ELEMENT_BASE: i64 = 33_000;

/// Vector dimension count encoded in `type_oid`, if it is a vector OID.
#[must_use]
pub fn vector_dimensions_for_oid(type_oid: i64) -> Option<usize> {
    let dimensions = usize::try_from(type_oid.checked_sub(VECTOR_OID_BASE)?).ok()?;
    (1..=VECTOR_OID_MAX_DIMENSIONS)
        .contains(&dimensions)
        .then_some(dimensions)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Null,
    SmallInt,
    Int,
    BigInt,
    Float,
    Boolean,
    Text,
    Char { length: Option<u32> },
    Varchar { length: Option<u32> },
    Uuid,
    Bytea,
    Date,
    Time,
    Timestamp,
    Vector(usize),
    Json,
    Array(Box<DataType>),
}

impl DataType {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn parse_sql(raw: &str) -> Result<Self, String> {
        parse_sql_type(raw)
    }

    #[must_use]
    pub fn type_name(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::SmallInt => "smallint".to_string(),
            Self::Int => "int".to_string(),
            Self::BigInt => "bigint".to_string(),
            Self::Float => "float".to_string(),
            Self::Boolean => "boolean".to_string(),
            Self::Text => "text".to_string(),
            Self::Char { length } => match length {
                Some(length) => format!("char({length})"),
                None => "char".to_string(),
            },
            Self::Varchar { length } => match length {
                Some(length) => format!("varchar({length})"),
                None => "varchar".to_string(),
            },
            Self::Uuid => "uuid".to_string(),
            Self::Bytea => "bytea".to_string(),
            Self::Date => "date".to_string(),
            Self::Time => "time".to_string(),
            Self::Timestamp => "timestamp".to_string(),
            Self::Vector(dimensions) => format!("vector({dimensions})"),
            Self::Json => "json".to_string(),
            Self::Array(inner) => format!("{}[]", inner.type_name()),
        }
    }

    #[must_use]
    pub fn type_oid(&self) -> i64 {
        const OID_ARRAY_BASE: i64 = 34000;
        match type_metadata(self) {
            TypeMetadata::Scalar(metadata) => i64::from(metadata.oid),
            TypeMetadata::Vector(dimensions) => {
                VECTOR_OID_BASE + i64::try_from(dimensions).unwrap_or(0)
            }
            TypeMetadata::Array(inner) => {
                let element = match inner {
                    Self::Vector(dimensions) => {
                        LEGACY_VECTOR_ELEMENT_BASE + i64::try_from(*dimensions).unwrap_or(0)
                    }
                    other => other.type_oid(),
                };
                OID_ARRAY_BASE + element % 10000
            }
        }
    }

    #[must_use]
    pub fn typlen(&self) -> i16 {
        match type_metadata(self) {
            TypeMetadata::Scalar(metadata) => metadata.typlen,
            TypeMetadata::Vector(_) | TypeMetadata::Array(_) => -1,
        }
    }

    #[must_use]
    pub fn atttypmod(&self) -> i32 {
        match self {
            Self::Varchar { length } => {
                let length = *length;
                length
                    .and_then(|length| i32::try_from(length).ok())
                    .and_then(|length| length.checked_add(4))
                    .unwrap_or(-1)
            }
            Self::Char { length } => length
                .and_then(|length| length.checked_add(4))
                .and_then(|length| i32::try_from(length).ok())
                .unwrap_or(-1),
            _ => -1,
        }
    }
}

fn parse_sql_type(raw: &str) -> Result<DataType, String> {
    let raw = raw.trim();
    if raw.ends_with("[]") {
        let inner = parse_sql_type(raw.strip_suffix("[]").unwrap_or(raw).trim())?;
        if matches!(inner, DataType::Array(_)) {
            return Err("array-of-array types are not supported".to_string());
        }
        return Ok(DataType::Array(Box::new(inner)));
    }

    let lower = raw.to_lowercase();
    if let Some(local_name) = lower.strip_prefix("pg_catalog.") {
        let name = local_name
            .split_once('(')
            .map_or(local_name, |(name, _)| name.trim());
        let known_builtin = super::builtin::BUILTIN_TYPES.iter().any(|metadata| {
            metadata.name == name
                // UNKNOWN and unconstrained BPCHAR require cast semantics
                // that the existing scalar runtime does not yet provide.
                && !matches!(
                    metadata.kind,
                    super::builtin::BuiltinKind::Unknown | super::builtin::BuiltinKind::Bpchar
                )
        });
        if !known_builtin {
            return Err(format!("unsupported data type '{raw}'"));
        }
        return parse_sql_type(local_name);
    }
    if lower.split_whitespace().eq(["double", "precision"]) {
        return Ok(DataType::Float);
    }
    let (base, modifier) = lower
        .split_once('(')
        .map_or((lower.as_str(), ""), |(base, _)| {
            (base, &lower[base.len()..])
        });
    let canonical = if base.split_whitespace().eq(["character"])
        || base.split_whitespace().eq(["national", "character"])
        || base.split_whitespace().eq(["nchar"])
    {
        Some("char")
    } else if base.split_whitespace().eq(["character", "varying"])
        || base.split_whitespace().eq(["char", "varying"])
        || base
            .split_whitespace()
            .eq(["national", "character", "varying"])
        || base.split_whitespace().eq(["nchar", "varying"])
    {
        Some("varchar")
    } else {
        None
    };
    if let Some(canonical) = canonical {
        return parse_sql_type(&format!("{canonical}{modifier}"));
    }
    if let Some(temporal) = lower.strip_suffix(" without time zone") {
        if temporal.starts_with("timestamp") || temporal.starts_with("time") {
            return parse_sql_type(temporal);
        }
    }
    if let Some(inner) = lower.strip_prefix("vector(") {
        let Some(inner) = inner.strip_suffix(')') else {
            return Err(format!("invalid vector type '{raw}'"));
        };
        let dimensions = inner
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid VECTOR dimension '{raw}'"))?;
        if dimensions == 0 {
            return Err(format!("invalid VECTOR dimension '{raw}'"));
        }
        return Ok(DataType::Vector(dimensions));
    }

    if let Some(length) = parse_string_type_with_length(&lower, "char") {
        return length;
    }

    if let Some(length) = parse_string_type_with_length(&lower, "varchar") {
        return length;
    }

    if parse_type_with_ignored_precision(&lower, "timestamp")? {
        return Ok(DataType::Timestamp);
    }

    if parse_type_with_ignored_precision(&lower, "time")? {
        return Ok(DataType::Time);
    }

    match lower.as_str() {
        "null" => Ok(DataType::Null),
        "smallint" | "int2" => Ok(DataType::SmallInt),
        "int" | "integer" | "int4" => Ok(DataType::Int),
        "bigint" | "int8" => Ok(DataType::BigInt),
        "float" | "float8" | "double" | "numeric" | "decimal" => Ok(DataType::Float),
        "boolean" | "bool" => Ok(DataType::Boolean),
        "text" | "string" => Ok(DataType::Text),
        "uuid" => Ok(DataType::Uuid),
        "bytea" => Ok(DataType::Bytea),
        "date" => Ok(DataType::Date),
        "time" => Ok(DataType::Time),
        "timestamp" => Ok(DataType::Timestamp),
        "json" | "jsonb" => Ok(DataType::Json),
        _ => Err(format!("unsupported data type '{raw}'")),
    }
}

fn parse_type_with_ignored_precision(raw: &str, kind: &str) -> Result<bool, String> {
    let Some(rest) = raw.strip_prefix(&format!("{kind}(")) else {
        return Ok(false);
    };
    let Some(rest) = rest.strip_suffix(')') else {
        return Err(format!("unsupported modifier in '{raw}'"));
    };
    let precision = rest
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("invalid {kind} precision '{raw}'"))?;
    if precision > 6 {
        return Err(format!("{kind} precision cannot exceed 6"));
    }
    Ok(true)
}

fn parse_string_type_with_length(raw: &str, kind: &str) -> Option<Result<DataType, String>> {
    if raw == kind {
        return if kind == "char" {
            Some(Ok(DataType::Char { length: Some(1) }))
        } else {
            Some(Ok(DataType::Varchar { length: None }))
        };
    }

    let rest = raw.strip_prefix(&format!("{kind}("))?;
    let Some(rest) = rest.strip_suffix(')') else {
        return Some(Err(format!("unsupported modifier in '{raw}'")));
    };

    let Ok(length) = rest.trim().parse::<u32>() else {
        return Some(Err(format!("invalid {kind} length '{raw}'")));
    };
    if length == 0 {
        return Some(Err(format!("{kind} length cannot be zero")));
    }

    if kind == "char" {
        Some(Ok(DataType::Char {
            length: Some(length),
        }))
    } else {
        Some(Ok(DataType::Varchar {
            length: Some(length),
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub fields: Vec<FieldSchema>,
}

impl Schema {
    #[must_use]
    pub fn vector_fields(&self) -> Vec<&FieldSchema> {
        self.fields
            .iter()
            .filter(|f| matches!(f.data_type, DataType::Vector(_)))
            .collect()
    }
}

impl std::iter::FromIterator<(String, DataType)> for Schema {
    fn from_iter<T: IntoIterator<Item = (String, DataType)>>(iter: T) -> Self {
        Self {
            fields: iter
                .into_iter()
                .map(|(name, data_type)| FieldSchema {
                    name,
                    data_type,
                    nullable: true,
                })
                .collect(),
        }
    }
}

impl Schema {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&FieldSchema> {
        self.fields.iter().find(|f| f.name == name)
    }
}
