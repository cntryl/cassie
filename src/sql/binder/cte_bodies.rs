//! Data-modifying `WITH` bodies (`INSERT|UPDATE|DELETE|MERGE ... RETURNING`).
//!
//! A data-modifying CTE exposes its `RETURNING` list as its columns, so its
//! names and types are those of `SELECT <returning> FROM <target>`. The
//! statement that owns the `WITH` list keeps the bound data-modifying bodies,
//! which the executor runs exactly once.
use std::borrow::Cow;

use super::{
    bind_delete, bind_insert, bind_update, BindingContext, CassieError, Catalog,
    CommonTableExpression, CteQuery, ParsedStatement, QuerySource, QueryStatement, SelectStatement,
};
use crate::sql::ast::{DeleteStatement, InsertStatement, SelectItem, UpdateStatement};

/// The query whose output a CTE body exposes: the body itself, or the
/// `RETURNING` view of a data-modifying body.
pub(crate) fn select_view(statement: &ParsedStatement) -> Option<Cow<'_, SelectStatement>> {
    let (table, alias, returning, joined) = match &statement.statement {
        QueryStatement::Select(select) => return Some(Cow::Borrowed(select)),
        QueryStatement::Insert(insert) => (
            &insert.table,
            insert.alias.as_deref(),
            &insert.returning,
            None,
        ),
        QueryStatement::Update(update) => (
            &update.table,
            update.alias.as_deref(),
            &update.returning,
            update.from.as_ref().map(|from| &from.source),
        ),
        QueryStatement::Delete(delete) => (
            &delete.table,
            delete.alias.as_deref(),
            &delete.returning,
            delete.using.as_ref().map(|using| &using.source),
        ),
        QueryStatement::Merge(merge) => (
            &merge.table,
            merge.alias.as_deref(),
            &merge.returning,
            Some(&merge.source.source),
        ),
        _ => return None,
    };
    if returning.is_empty() {
        return None;
    }
    let target = QuerySource::Collection(table.clone());
    let target = match alias {
        Some(alias) => QuerySource::Aliased {
            source: Box::new(target),
            alias: alias.to_string(),
            column_aliases: Vec::new(),
        },
        None => target,
    };
    let source = match joined {
        Some(source) => QuerySource::Join {
            left: Box::new(target),
            right: Box::new(source.clone()),
            kind: crate::sql::ast::JoinKind::Cross,
            on: crate::sql::ast::Expr::BoolLiteral(true),
        },
        None => target,
    };
    Some(Cow::Owned(SelectStatement {
        source,
        ctes: Vec::new(),
        recursive: false,
        distinct: false,
        distinct_on: Vec::new(),
        projection: returning.iter().map(view_item).collect(),
        filter: None,
        group_by: Vec::new(),
        having: None,
        order: Vec::new(),
        limit: None,
        offset: None,
        set: None,
    }))
}

/// `merge_action()` is a row-image column of the mutation, typed TEXT.
fn view_item(item: &SelectItem) -> SelectItem {
    match item {
        SelectItem::Function { function, alias }
            if function.name.eq_ignore_ascii_case("merge_action") =>
        {
            SelectItem::Expr {
                expr: crate::sql::ast::Expr::Cast {
                    expr: Box::new(crate::sql::ast::Expr::StringLiteral(String::new())),
                    data_type: crate::types::DataType::Text,
                    source_type: None,
                    identity: crate::sql::ast::CastIdentity::Scalar,
                },
                alias: alias.clone().or_else(|| Some("merge_action".into())),
            }
        }
        other => other.clone(),
    }
}

/// Returns whether `statement` modifies data.
pub(crate) fn is_modifying(statement: &ParsedStatement) -> bool {
    matches!(
        statement.statement,
        QueryStatement::Insert(_)
            | QueryStatement::Update(_)
            | QueryStatement::Delete(_)
            | QueryStatement::Merge(_)
    )
}

/// Returns whether `cte` is a data-modifying CTE.
pub(crate) fn cte_is_modifying(cte: &CommonTableExpression) -> bool {
    matches!(&cte.query, CteQuery::Simple(statement) if is_modifying(statement))
}

/// Binds the data-modifying bodies an INSERT, UPDATE or DELETE keeps; its
/// other `WITH` queries are already carried by the queries that read them.
fn bind_owned(
    ctes: Vec<CommonTableExpression>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<Vec<CommonTableExpression>, CassieError> {
    let mut bound = Vec::new();
    for mut cte in ctes {
        if !cte_is_modifying(&cte) {
            continue;
        }
        if let CteQuery::Simple(statement) = cte.query {
            let statement =
                super::bind_statement(*statement, catalog, &super::CteScope::new(None), context)?;
            cte.query = CteQuery::Simple(Box::new(statement));
        }
        bound.push(cte);
    }
    Ok(bound)
}

pub(super) fn insert(
    statement: InsertStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<InsertStatement, CassieError> {
    // A joined source binds against the inherited definitions too.
    let ctes = statement.ctes.clone();
    let mut bound = bind_insert(statement, catalog, context)?;
    bound.ctes = bind_owned(ctes, catalog, context)?;
    Ok(bound)
}

pub(super) fn update(
    statement: UpdateStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<UpdateStatement, CassieError> {
    // A joined source binds against the inherited definitions too.
    let ctes = statement.ctes.clone();
    let mut bound = bind_update(statement, catalog, context)?;
    bound.ctes = bind_owned(ctes, catalog, context)?;
    Ok(bound)
}

pub(super) fn delete(
    statement: DeleteStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DeleteStatement, CassieError> {
    // A joined source binds against the inherited definitions too.
    let ctes = statement.ctes.clone();
    let mut bound = bind_delete(statement, catalog, context)?;
    bound.ctes = bind_owned(ctes, catalog, context)?;
    Ok(bound)
}
