//! Whether a planned query reads a virtual catalog relation, which bypasses result caching.

use crate::sql::ast::QueryStatement;

pub(super) fn logical_plan_uses_virtual_catalog(
    plan: &crate::planner::logical::LogicalPlan,
) -> bool {
    query_source_uses_virtual_catalog(&plan.source)
        || plan.ctes.iter().any(|cte| match &cte.query {
            crate::sql::ast::CteQuery::Simple(statement) => {
                parsed_statement_uses_virtual_catalog(statement)
            }
            crate::sql::ast::CteQuery::Recursive {
                base, recursive, ..
            } => {
                parsed_statement_uses_virtual_catalog(base)
                    || parsed_statement_uses_virtual_catalog(recursive)
            }
        })
        || plan.set.as_ref().is_some_and(|set| {
            query_source_uses_virtual_catalog(&set.right.source)
                || set.right.ctes.iter().any(|cte| match &cte.query {
                    crate::sql::ast::CteQuery::Simple(statement) => {
                        parsed_statement_uses_virtual_catalog(statement)
                    }
                    crate::sql::ast::CteQuery::Recursive {
                        base, recursive, ..
                    } => {
                        parsed_statement_uses_virtual_catalog(base)
                            || parsed_statement_uses_virtual_catalog(recursive)
                    }
                })
        })
}

fn parsed_statement_uses_virtual_catalog(statement: &crate::sql::ast::ParsedStatement) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => query_source_uses_virtual_catalog(&select.source),
        _ => false,
    }
}

fn query_source_uses_virtual_catalog(source: &crate::sql::ast::QuerySource) -> bool {
    match source {
        crate::sql::ast::QuerySource::Aliased { source, .. }
        | crate::sql::ast::QuerySource::TableSample { source, .. } => {
            query_source_uses_virtual_catalog(source)
        }
        crate::sql::ast::QuerySource::Collection(name) => {
            crate::catalog::virtual_views::schema(name).is_some()
        }
        crate::sql::ast::QuerySource::Subquery { select, .. } => {
            query_source_uses_virtual_catalog(&select.source)
        }
        crate::sql::ast::QuerySource::Join { left, right, .. } => {
            query_source_uses_virtual_catalog(left) || query_source_uses_virtual_catalog(right)
        }
        crate::sql::ast::QuerySource::Cte(_)
        | crate::sql::ast::QuerySource::Values { .. }
        | crate::sql::ast::QuerySource::SingleRow
        | crate::sql::ast::QuerySource::TableFunction { .. } => false,
    }
}
