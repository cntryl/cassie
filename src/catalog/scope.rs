use serde::{Deserialize, Serialize};

pub const DEFAULT_SCHEMA: &str = "public";
pub const PG_CATALOG_SCHEMA: &str = "pg_catalog";
pub const INFORMATION_SCHEMA: &str = "information_schema";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseMeta {
    pub name: String,
    pub description: Option<String>,
}

impl DatabaseMeta {
    #[must_use]
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self {
            name: name.into(),
            description,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SchemaId {
    pub database: String,
    pub schema: String,
}

impl SchemaId {
    #[must_use]
    pub fn new(database: impl Into<String>, schema: impl Into<String>) -> Self {
        Self {
            database: database.into(),
            schema: schema.into(),
        }
    }

    #[must_use]
    pub fn canonical_name(&self) -> String {
        canonical_schema_name(&self.database, &self.schema)
    }

    #[must_use]
    pub fn relation(&self, name: impl Into<String>) -> RelationId {
        RelationId::new(self.database.clone(), self.schema.clone(), name)
    }

    #[must_use]
    pub fn parse_canonical(raw: &str) -> Option<Self> {
        let ParsedName::SchemaQualified { schema, name } = parse_name(raw).ok()? else {
            return None;
        };
        Some(Self::new(schema, name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelationId {
    pub database: String,
    pub schema: String,
    pub name: String,
}

impl RelationId {
    #[must_use]
    pub fn new(
        database: impl Into<String>,
        schema: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            database: database.into(),
            schema: schema.into(),
            name: name.into(),
        }
    }

    #[must_use]
    pub fn canonical_name(&self) -> String {
        canonical_relation_name(&self.database, &self.schema, &self.name)
    }

    #[must_use]
    pub fn schema_id(&self) -> SchemaId {
        SchemaId::new(self.database.clone(), self.schema.clone())
    }

    #[must_use]
    pub fn parse_canonical(raw: &str) -> Option<Self> {
        let ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        } = parse_name(raw).ok()?
        else {
            return None;
        };
        Some(Self::new(database, schema, name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedName {
    Unqualified(String),
    SchemaQualified { schema: String, name: String },
    DatabaseQualified {
        database: String,
        schema: String,
        name: String,
    },
}

#[must_use]
pub fn canonical_schema_name(database: &str, schema: &str) -> String {
    format!("{database}.{schema}")
}

#[must_use]
pub fn canonical_relation_name(database: &str, schema: &str, name: &str) -> String {
    format!("{database}.{schema}.{name}")
}

#[must_use]
pub fn local_name(raw: &str) -> String {
    match parse_name(raw) {
        Ok(ParsedName::Unqualified(name))
        | Ok(ParsedName::SchemaQualified { name, .. })
        | Ok(ParsedName::DatabaseQualified { name, .. }) => name,
        Err(_) => raw.to_string(),
    }
}

#[must_use]
pub fn parent_schema(raw: &str) -> Option<SchemaId> {
    match parse_name(raw).ok()? {
        ParsedName::DatabaseQualified {
            database,
            schema,
            name: _,
        } => Some(SchemaId::new(database, schema)),
        ParsedName::SchemaQualified { schema, name } => Some(SchemaId::new(schema, name)),
        ParsedName::Unqualified(_) => None,
    }
}

#[must_use]
pub fn derive_scoped_name(base: &str, derived_local_name: impl FnOnce(&str) -> String) -> String {
    if let Some(parent) = parent_schema(base) {
        return parent.relation(derived_local_name(&local_name(base))).canonical_name();
    }
    derived_local_name(base)
}

#[must_use]
pub fn relation_database_name(raw: &str) -> Option<String> {
    RelationId::parse_canonical(raw).map(|relation| relation.database)
}

#[must_use]
pub fn relation_schema_name(raw: &str) -> String {
    RelationId::parse_canonical(raw)
        .map(|relation| relation.schema)
        .unwrap_or_else(|| DEFAULT_SCHEMA.to_string())
}

#[must_use]
pub fn schema_database_name(raw: &str) -> Option<String> {
    SchemaId::parse_canonical(raw).map(|schema| schema.database)
}

#[must_use]
pub fn relation_belongs_to_database(raw: &str, database: &str) -> bool {
    relation_database_name(raw)
        .is_none_or(|name| name.eq_ignore_ascii_case(database))
}

#[must_use]
pub fn schema_belongs_to_database(raw: &str, database: &str) -> bool {
    schema_database_name(raw).is_none_or(|name| name.eq_ignore_ascii_case(database))
}

#[must_use]
pub fn name_matches(stored: &str, requested: &str) -> bool {
    if stored.eq_ignore_ascii_case(requested) {
        return true;
    }

    let Ok(stored_parts) = split_identifier_path(stored) else {
        return false;
    };
    let Ok(requested_parts) = split_identifier_path(requested) else {
        return false;
    };
    if requested_parts.len() > stored_parts.len() {
        return false;
    }

    stored_parts
        .iter()
        .rev()
        .zip(requested_parts.iter().rev())
        .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

#[must_use]
pub fn is_reserved_namespace(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        INFORMATION_SCHEMA | PG_CATALOG_SCHEMA | DEFAULT_SCHEMA
    )
}

#[must_use]
pub fn is_system_schema(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        INFORMATION_SCHEMA | PG_CATALOG_SCHEMA
    )
}

/// # Errors
///
/// Returns an error when a dotted identifier path is malformed.
pub fn split_identifier_path(raw: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = raw.trim().chars().peekable();
    let mut in_quotes = false;

    while let Some(character) = chars.next() {
        if in_quotes {
            if character == '"' {
                if chars.peek().is_some_and(|next| *next == '"') {
                    current.push('"');
                    let _ = chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(character);
            }
            continue;
        }

        match character {
            '"' => in_quotes = true,
            '.' => {
                let part = current.trim();
                if part.is_empty() {
                    return Err(format!("invalid qualified name '{raw}'"));
                }
                parts.push(part.to_string());
                current.clear();
            }
            _ => current.push(character),
        }
    }

    if in_quotes {
        return Err(format!("unterminated quoted identifier '{raw}'"));
    }

    let part = current.trim();
    if part.is_empty() {
        return Err(format!("invalid qualified name '{raw}'"));
    }
    parts.push(part.to_string());
    Ok(parts)
}

/// # Errors
///
/// Returns an error when the identifier path is malformed or too deep.
pub fn parse_name(raw: &str) -> Result<ParsedName, String> {
    let parts = split_identifier_path(raw)?;
    match parts.as_slice() {
        [name] => Ok(ParsedName::Unqualified(name.clone())),
        [schema, name] => Ok(ParsedName::SchemaQualified {
            schema: schema.clone(),
            name: name.clone(),
        }),
        [database, schema, name] => Ok(ParsedName::DatabaseQualified {
            database: database.clone(),
            schema: schema.clone(),
            name: name.clone(),
        }),
        _ => Err(format!("unsupported qualified name '{raw}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_relation_name, parse_name, split_identifier_path, ParsedName, RelationId,
        SchemaId,
    };

    #[test]
    fn should_parse_quoted_identifier_paths() {
        // Arrange
        let raw = r#""tenant.db"."reporting"."orders.archive""#;

        // Act
        let parsed = split_identifier_path(raw).expect("identifier path");

        // Assert
        assert_eq!(parsed, vec!["tenant.db", "reporting", "orders.archive"]);
    }

    #[test]
    fn should_parse_canonical_relation_name() {
        // Arrange
        let raw = "tenant_db.reporting.orders";

        // Act
        let parsed = RelationId::parse_canonical(raw).expect("relation id");

        // Assert
        assert_eq!(parsed, RelationId::new("tenant_db", "reporting", "orders"));
        assert_eq!(parsed.schema_id(), SchemaId::new("tenant_db", "reporting"));
        assert_eq!(parsed.canonical_name(), canonical_relation_name("tenant_db", "reporting", "orders"));
    }

    #[test]
    fn should_distinguish_name_depths() {
        // Arrange
        let one = parse_name("orders").expect("unqualified");
        let two = parse_name("reporting.orders").expect("schema-qualified");
        let three = parse_name("tenant_db.reporting.orders").expect("database-qualified");

        // Assert
        assert_eq!(one, ParsedName::Unqualified("orders".to_string()));
        assert_eq!(
            two,
            ParsedName::SchemaQualified {
                schema: "reporting".to_string(),
                name: "orders".to_string(),
            }
        );
        assert_eq!(
            three,
            ParsedName::DatabaseQualified {
                database: "tenant_db".to_string(),
                schema: "reporting".to_string(),
                name: "orders".to_string(),
            }
        );
    }
}
