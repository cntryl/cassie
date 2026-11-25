//! Retain supplied parameter identities before ordinary source binding.
use crate::app::CassieError;
use crate::sql::ast::{
    CteQuery, Expr, ParsedStatement, QuerySource, QueryStatement, SelectStatement,
};

pub(super) fn bind(statement: &mut ParsedStatement, oids: &[i32]) -> Result<(), CassieError> {
    match &mut statement.statement {
        QueryStatement::Select(query) => select(query, oids)?,
        QueryStatement::Explain(explain) => bind(&mut explain.statement, oids)?,
        QueryStatement::Insert(insert) => {
            if let crate::sql::ast::InsertSource::Select(query) = &mut insert.source {
                select(query, oids)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn select(query: &mut SelectStatement, oids: &[i32]) -> Result<(), CassieError> {
    source(&mut query.source, oids)?;
    for cte in &mut query.ctes {
        match &mut cte.query {
            CteQuery::Simple(statement) => bind(statement, oids)?,
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                bind(base, oids)?;
                bind(recursive, oids)?;
            }
        }
    }
    if let Some(set) = &mut query.set {
        select(&mut set.right, oids)?;
    }
    Ok(())
}
fn source(query: &mut QuerySource, oids: &[i32]) -> Result<(), CassieError> {
    match query {
        QuerySource::Values { rows } => {
            for expr in rows.iter_mut().flatten() {
                expression(expr, oids)?;
            }
        }
        QuerySource::Aliased { source: child, .. } => source(child, oids)?,
        QuerySource::Subquery { select: query, .. } => select(query, oids)?,
        QuerySource::Join { left, right, .. } => {
            source(left, oids)?;
            source(right, oids)?;
        }
        _ => {}
    }
    Ok(())
}
fn expression(expr: &mut Expr, oids: &[i32]) -> Result<(), CassieError> {
    if let Expr::Param(index) = expr {
        if let Some(oid) = oids.get(*index) {
            if *oid == 700 {
                return Err(CassieError::Unsupported(
                    "REAL VALUES parameters are not yet supported".into(),
                ));
            }
            if !matches!(*oid, 0 | 705)
                && super::super::inference::data_type_for_parameter_oid(*oid).is_none()
            {
                return Err(CassieError::Unsupported(format!(
                    "VALUES parameter type OID {oid} is not yet admitted"
                )));
            }
            if let Some(data_type) = super::super::inference::data_type_for_parameter_oid(*oid) {
                let original = std::mem::replace(expr, Expr::Null);
                *expr = Expr::Cast {
                    expr: Box::new(original),
                    data_type,
                    source_type: None,
                    identity: crate::sql::ast::CastIdentity::Scalar,
                };
            }
        }
        return Ok(());
    }
    expr.try_visit_children_mut(|child| expression(child, oids))
}
