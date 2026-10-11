use super::command_output::empty as empty_command;
use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    AlterRoleStatement, CreateRoleStatement, DatabaseConnectPrivilegeStatement, DropRoleStatement,
};

pub(super) fn create_role(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &CreateRoleStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let database = session
        .and_then(CassieSession::current_database)
        .unwrap_or(cassie.default_database.as_str());
    let output = empty_command("CREATE ROLE", controls)?;
    cassie
        .create_role_in_database(
            &statement.name,
            statement.login,
            statement.password.clone(),
            statement.if_not_exists,
            database,
        )
        .map_err(QueryError::Cassie)?;

    Ok(output)
}

pub(super) fn alter_role(
    cassie: &Cassie,
    statement: &AlterRoleStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("ALTER ROLE", controls)?;
    cassie
        .alter_role(
            &statement.name,
            statement.login,
            statement.password.clone(),
        )
        .map_err(QueryError::Cassie)?;

    Ok(output)
}

pub(super) fn drop_role(
    cassie: &Cassie,
    statement: &DropRoleStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("DROP ROLE", controls)?;
    cassie
        .drop_role(&statement.name, statement.if_exists)
        .map_err(QueryError::Cassie)?;

    Ok(output)
}

pub(super) fn grant_database_connect(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &DatabaseConnectPrivilegeStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let actor = session.ok_or(QueryError::Cassie(crate::app::CassieError::Unauthorized))?;
    let output = empty_command("GRANT", controls)?;
    cassie
        .grant_role_database_access(actor, &statement.role, &statement.database)
        .map_err(QueryError::Cassie)?;
    Ok(output)
}

pub(super) fn revoke_database_connect(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &DatabaseConnectPrivilegeStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let actor = session.ok_or(QueryError::Cassie(crate::app::CassieError::Unauthorized))?;
    let output = empty_command("REVOKE", controls)?;
    cassie
        .revoke_role_database_access(actor, &statement.role, &statement.database)
        .map_err(QueryError::Cassie)?;
    Ok(output)
}
