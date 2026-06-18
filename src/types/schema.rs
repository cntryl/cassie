use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Null,
    Int,
    Float,
    Boolean,
    Text,
    Uuid,
    Date,
    Time,
    Timestamp,
    Vector(usize),
    Json,
    Array(Box<DataType>),
}

impl DataType {
    pub fn parse_sql(raw: &str) -> Result<Self, String> {
        parse_sql_type(raw)
    }

    pub fn type_name(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Boolean => "boolean".to_string(),
            Self::Text => "text".to_string(),
            Self::Uuid => "uuid".to_string(),
            Self::Date => "date".to_string(),
            Self::Time => "time".to_string(),
            Self::Timestamp => "timestamp".to_string(),
            Self::Vector(dimensions) => format!("vector({dimensions})"),
            Self::Json => "json".to_string(),
            Self::Array(inner) => format!("{}[]", inner.type_name()),
        }
    }

    pub fn type_oid(&self) -> i64 {
        const OID_BOOL: i64 = 16;
        const OID_INT8: i64 = 20;
        const OID_TEXT: i64 = 25;
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
            Self::Int => OID_INT8,
            Self::Float => OID_FLOAT8,
            Self::Boolean => OID_BOOL,
            Self::Text => OID_TEXT,
            Self::Uuid => OID_UUID,
            Self::Date => OID_DATE,
            Self::Time => OID_TIME,
            Self::Timestamp => OID_TIMESTAMP,
            Self::Vector(dimensions) => OID_VECTOR_BASE + i64::try_from(*dimensions).unwrap_or(0),
            Self::Json => OID_JSON,
            Self::Array(inner) => OID_ARRAY_BASE + (inner.type_oid() % 10000),
        }
    }

    pub fn typlen(&self) -> i16 {
        match self {
            Self::Null => 0,
            Self::Int => 8,
            Self::Float => 8,
            Self::Boolean => 1,
            Self::Text => -1,
            Self::Uuid => 16,
            Self::Date => 4,
            Self::Time => 8,
            Self::Timestamp => 8,
            Self::Vector(_) => -1,
            Self::Json => -1,
            Self::Array(_) => -1,
        }
    }

    pub fn atttypmod(&self) -> i32 {
        match self {
            Self::Vector(_) => -1,
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

    match lower.as_str() {
        "null" => Ok(DataType::Null),
        "int" | "integer" => Ok(DataType::Int),
        "float" | "double" | "numeric" | "decimal" => Ok(DataType::Float),
        "boolean" | "bool" => Ok(DataType::Boolean),
        "text" | "string" | "varchar" => Ok(DataType::Text),
        "uuid" => Ok(DataType::Uuid),
        "date" => Ok(DataType::Date),
        "time" => Ok(DataType::Time),
        "timestamp" => Ok(DataType::Timestamp),
        "json" => Ok(DataType::Json),
        _ => Err(format!("unsupported data type '{raw}'")),
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
    pub fn field(&self, name: &str) -> Option<&FieldSchema> {
        self.fields.iter().find(|f| f.name == name)
    }
}
