use serde::{Deserialize, Serialize};

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
        const OID_BOOL: i64 = 16;
        const OID_INT2: i64 = 21;
        const OID_INT8: i64 = 20;
        const OID_INT4: i64 = 23;
        const OID_TEXT: i64 = 25;
        const OID_BYTEA: i64 = 17;
        const OID_BPCHAR: i64 = 1042;
        const OID_VARCHAR: i64 = 1043;
        const OID_JSON: i64 = 114;
        const OID_FLOAT8: i64 = 701;
        const OID_UUID: i64 = 2950;
        const OID_DATE: i64 = 1082;
        const OID_TIME: i64 = 1083;
        const OID_TIMESTAMP: i64 = 1114;
        const OID_VECTOR_BASE: i64 = 33000;
        const OID_UNKNOWN: i64 = 705;
        const OID_ARRAY_BASE: i64 = 34000;

        match self {
            Self::Null => OID_UNKNOWN,
            Self::SmallInt => OID_INT2,
            Self::Int => OID_INT4,
            Self::BigInt => OID_INT8,
            Self::Float => OID_FLOAT8,
            Self::Boolean => OID_BOOL,
            Self::Text => OID_TEXT,
            Self::Char { .. } => OID_BPCHAR,
            Self::Varchar { .. } => OID_VARCHAR,
            Self::Uuid => OID_UUID,
            Self::Bytea => OID_BYTEA,
            Self::Date => OID_DATE,
            Self::Time => OID_TIME,
            Self::Timestamp => OID_TIMESTAMP,
            Self::Vector(dimensions) => OID_VECTOR_BASE + i64::try_from(*dimensions).unwrap_or(0),
            Self::Json => OID_JSON,
            Self::Array(inner) => OID_ARRAY_BASE + (inner.type_oid() % 10000),
        }
    }

    #[must_use]
    pub fn typlen(&self) -> i16 {
        match self {
            Self::Null => 0,
            Self::SmallInt => 2,
            Self::Int | Self::Date => 4,
            Self::BigInt | Self::Float | Self::Time | Self::Timestamp => 8,
            Self::Boolean => 1,
            Self::Char { .. } | Self::Varchar { .. } | Self::Text | Self::Bytea | Self::Vector(_) | Self::Json | Self::Array(_) => -1,
            Self::Uuid => 16,
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
                .unwrap_or(1)
                .checked_add(4)
                .and_then(|length| i32::try_from(length).ok())
                .unwrap_or(5),
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
        "float" | "double" | "numeric" | "decimal" => Ok(DataType::Float),
        "boolean" | "bool" => Ok(DataType::Boolean),
        "text" | "string" | "varchar" => Ok(DataType::Text),
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
