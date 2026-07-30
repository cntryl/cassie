use crate::app::{Cassie, CassieError};
use crate::catalog::DatabaseMeta;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreateDatabaseRequest {
    pub name: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DatabaseSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl From<DatabaseMeta> for DatabaseSummary {
    fn from(database: DatabaseMeta) -> Self {
        Self {
            name: database.name,
            description: database.description,
        }
    }
}

pub(crate) fn list(cassie: &Cassie) -> Result<Vec<DatabaseSummary>, CassieError> {
    let mut databases = cassie.midge.list_databases()?;
    databases.sort_by_key(|database| database.name.to_ascii_lowercase());
    Ok(databases.into_iter().map(DatabaseSummary::from).collect())
}

pub(crate) fn create(cassie: &Cassie, body: &[u8]) -> Result<DatabaseSummary, CassieError> {
    let request: CreateDatabaseRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;
    let name = normalize_admin_database_name(&request.name)?;
    cassie
        .create_logical_database(&name, false)
        .map(DatabaseSummary::from)
}

fn normalize_admin_database_name(raw: &str) -> Result<String, CassieError> {
    let name = raw.trim();
    let mut characters = name.chars();
    let valid_start = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    let valid_rest =
        characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid_start || !valid_rest {
        return Err(CassieError::InvalidQuery(
            "database names must be unqualified SQL identifiers".to_string(),
        ));
    }
    Ok(name.to_ascii_lowercase())
}
