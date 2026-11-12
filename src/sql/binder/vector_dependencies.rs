use crate::sql::ast::{
    BinaryOp, CteQuery, Expr, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};
use crate::types::DataType;

pub(crate) fn select_uses_vector_extension(statement: &SelectStatement) -> bool {
    source_uses_vector_extension(&statement.source)
        || statement.projection.iter().any(item_uses_vector_extension)
        || statement
            .filter
            .as_ref()
            .is_some_and(expr_uses_vector_extension)
        || statement
            .having
            .as_ref()
            .is_some_and(expr_uses_vector_extension)
        || statement.distinct_on.iter().any(expr_uses_vector_extension)
        || statement.group_by.iter().any(expr_uses_vector_extension)
        || statement
            .order
            .iter()
            .any(|order| expr_uses_vector_extension(&order.expr))
        || statement
            .limit
            .as_ref()
            .is_some_and(expr_uses_vector_extension)
        || statement
            .offset
            .as_ref()
            .is_some_and(expr_uses_vector_extension)
        || statement
            .set
            .as_ref()
            .is_some_and(|set| select_uses_vector_extension(&set.right))
        || statement.ctes.iter().any(cte_uses_vector_extension)
}

fn cte_uses_vector_extension(cte: &crate::sql::ast::CommonTableExpression) -> bool {
    let query_uses_vector = match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_uses_vector_extension(statement),
        CteQuery::Recursive {
            base, recursive, ..
        } => {
            parsed_statement_uses_vector_extension(base)
                || parsed_statement_uses_vector_extension(recursive)
        }
    };
    query_uses_vector
        || cte.cycle.as_ref().is_some_and(|cycle| {
            expr_uses_vector_extension(&cycle.mark_value)
                || expr_uses_vector_extension(&cycle.default_value)
        })
}

fn parsed_statement_uses_vector_extension(statement: &ParsedStatement) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => select_uses_vector_extension(select),
        _ => false,
    }
}

fn source_uses_vector_extension(source: &QuerySource) -> bool {
    match source {
        QuerySource::Values { rows } => rows.iter().flatten().any(expr_uses_vector_extension),
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            source_uses_vector_extension(source)
                || expr_uses_vector_extension(rate)
                || repeatable.as_ref().is_some_and(expr_uses_vector_extension)
        }
        QuerySource::Aliased { source, .. } => source_uses_vector_extension(source),
        QuerySource::TableFunction { function, .. } => {
            function.expressions().any(expr_uses_vector_extension)
        }
        QuerySource::Subquery { select, .. } => select_uses_vector_extension(select),
        QuerySource::Join {
            left, right, on, ..
        } => {
            source_uses_vector_extension(left)
                || source_uses_vector_extension(right)
                || expr_uses_vector_extension(on)
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn item_uses_vector_extension(item: &SelectItem) -> bool {
    match item {
        SelectItem::Expr { expr, .. } => expr_uses_vector_extension(expr),
        SelectItem::Function { function, .. } => {
            function.expressions().any(expr_uses_vector_extension)
        }
        SelectItem::WindowFunction { function, .. } => {
            function.args.iter().any(expr_uses_vector_extension)
                || function
                    .filter
                    .as_deref()
                    .is_some_and(expr_uses_vector_extension)
                || function.partition_by.iter().any(expr_uses_vector_extension)
                || function
                    .order_by
                    .iter()
                    .any(|order| expr_uses_vector_extension(&order.expr))
        }
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
    }
}

fn expr_uses_vector_extension(expression: &Expr) -> bool {
    expression.any_descendant_or_self(&mut |candidate| match candidate {
        Expr::Binary {
            op: BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot,
            ..
        } => true,
        Expr::Cast { data_type, .. } => data_type_uses_vector(data_type),
        Expr::Exists(statement) => parsed_statement_uses_vector_extension(statement),
        Expr::Subquery(subquery) => parsed_statement_uses_vector_extension(&subquery.query),
        _ => false,
    })
}

pub(crate) fn data_type_uses_vector(data_type: &DataType) -> bool {
    match data_type {
        DataType::Vector(_) => true,
        DataType::Array(element) => data_type_uses_vector(element),
        _ => false,
    }
}
