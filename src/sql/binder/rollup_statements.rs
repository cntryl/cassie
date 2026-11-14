//! Binding of the rollup and retention-policy maintenance statements:
//! `REFRESH ROLLUP`, `DROP ROLLUP` and `DROP RETENTION POLICY`.

use super::{
    parsed_statement, resolve_existing_name, BindingContext, CassieError, Catalog,
    CatalogObjectKind, ParsedStatement, QueryStatement,
};

pub(super) fn bind_refresh_rollup_statement(
    statement: crate::sql::ast::RefreshRollupStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::RefreshRollupStatement { name } = statement;
    let name = resolve_existing_name(name.trim(), context, |name| {
        catalog.get_rollup(name).is_some()
    })?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "REFRESH ROLLUP requires a name".into(),
        ));
    }
    if catalog.get_rollup(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Rollup,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::RefreshRollup(crate::sql::ast::RefreshRollupStatement { name }),
    ))
}

pub(super) fn bind_drop_rollup_statement(
    statement: crate::sql::ast::DropRollupStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::DropRollupStatement { name, if_exists } = statement;
    let name = resolve_existing_name(name.trim(), context, |name| {
        catalog.get_rollup(name).is_some()
    })?;
    if name.is_empty() {
        return Err(CassieError::Planner("DROP ROLLUP requires a name".into()));
    }
    if !if_exists && catalog.get_rollup(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Rollup,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropRollup(crate::sql::ast::DropRollupStatement { name, if_exists }),
    ))
}

pub(super) fn bind_drop_retention_policy_statement(
    statement: crate::sql::ast::DropRetentionPolicyStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::DropRetentionPolicyStatement { name, if_exists } = statement;
    let name = resolve_existing_name(name.trim(), context, |name| {
        catalog.get_retention_policy(name).is_some()
    })?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP RETENTION POLICY requires a name".into(),
        ));
    }
    if !if_exists && catalog.get_retention_policy(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::RetentionPolicy,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropRetentionPolicy(crate::sql::ast::DropRetentionPolicyStatement {
            name,
            if_exists,
        }),
    ))
}
