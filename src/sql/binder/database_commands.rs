//! Database DDL binding and normalized catalog identities.
use super::{
    normalize_database_name, parsed_statement, CassieError, Catalog, CatalogObjectKind,
    CreateDatabaseStatement, DropDatabaseStatement, ParsedStatement, QueryStatement,
};

pub(super) fn bind_create_database_statement(
    statement: CreateDatabaseStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    let CreateDatabaseStatement {
        name,
        if_not_exists,
    } = statement;
    let name = normalize_database_name(name.trim())?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE DATABASE requires a name".into(),
        ));
    }
    if !if_not_exists && catalog.database_exists(&name) {
        return Err(CassieError::Planner(format!(
            "database '{name}' already exists"
        )));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CreateDatabase(CreateDatabaseStatement {
            name,
            if_not_exists,
        }),
    ))
}

pub(super) fn bind_drop_database_statement(
    statement: DropDatabaseStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    let DropDatabaseStatement { name, if_exists } = statement;
    let name = normalize_database_name(name.trim())?;
    if name.is_empty() {
        return Err(CassieError::Planner("DROP DATABASE requires a name".into()));
    }
    if !if_exists && !catalog.database_exists(&name) {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Database,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropDatabase(DropDatabaseStatement { name, if_exists }),
    ))
}
