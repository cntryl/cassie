use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::sql::ast::{
    AlterRoleStatement, CreateRoleStatement, DatabaseConnectPrivilegeStatement, DropRoleStatement,
};

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

pub(super) fn grant_database_connect(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &DatabaseConnectPrivilegeStatement,
) -> Result<QueryResult, QueryError> {
    let actor = session.ok_or(QueryError::Cassie(crate::app::CassieError::Unauthorized))?;
    cassie
        .grant_role_database_access(actor, &statement.role, &statement.database)
        .map_err(QueryError::Cassie)?;
    Ok(empty_command("GRANT"))
}

pub(super) fn revoke_database_connect(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &DatabaseConnectPrivilegeStatement,
) -> Result<QueryResult, QueryError> {
    let actor = session.ok_or(QueryError::Cassie(crate::app::CassieError::Unauthorized))?;
    cassie
        .revoke_role_database_access(actor, &statement.role, &statement.database)
        .map_err(QueryError::Cassie)?;
    Ok(empty_command("REVOKE"))
}

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
