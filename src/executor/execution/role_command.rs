use super::{Cassie, QueryError, QueryResult};
use crate::sql::ast::{AlterRoleStatement, CreateRoleStatement, DropRoleStatement};

pub(super) fn create_role(
    cassie: &Cassie,
    statement: &CreateRoleStatement,
) -> Result<QueryResult, QueryError> {
    cassie
        .create_role(
            &statement.name,
            statement.login,
            statement.password.clone(),
            statement.if_not_exists,
        )
        .map_err(QueryError::Cassie)?;

    Ok(empty_command("CREATE ROLE"))
}

pub(super) fn alter_role(
    cassie: &Cassie,
    statement: &AlterRoleStatement,
) -> Result<QueryResult, QueryError> {
    cassie
        .alter_role(&statement.name, statement.login, statement.password.clone())
        .map_err(QueryError::Cassie)?;

    Ok(empty_command("ALTER ROLE"))
}

pub(super) fn drop_role(
    cassie: &Cassie,
    statement: &DropRoleStatement,
) -> Result<QueryResult, QueryError> {
    cassie
        .drop_role(&statement.name, statement.if_exists)
        .map_err(QueryError::Cassie)?;

    Ok(empty_command("DROP ROLE"))
}

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
