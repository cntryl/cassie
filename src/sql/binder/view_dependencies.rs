use std::collections::BTreeSet;

use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, ParsedStatement, QuerySource, QueryStatement,
    SelectItem, SelectStatement,
};

pub(crate) fn select_referenced_relations(statement: &SelectStatement) -> Vec<String> {
    let mut relations = BTreeSet::new();
    collect_select(statement, &mut relations);
    relations.into_iter().collect()
}

fn collect_select(statement: &SelectStatement, relations: &mut BTreeSet<String>) {
    collect_source(&statement.source, relations);
    for item in &statement.projection {
        collect_item(item, relations);
    }
    for expression in statement
        .filter
        .iter()
        .chain(&statement.distinct_on)
        .chain(&statement.group_by)
        .chain(statement.having.iter())
        .chain(statement.limit.iter())
        .chain(statement.offset.iter())
    {
        collect_expression(expression, relations);
    }
    for order in &statement.order {
        collect_expression(&order.expr, relations);
    }
    if let Some(set) = &statement.set {
        collect_select(&set.right, relations);
    }
    for cte in &statement.ctes {
        collect_cte(cte, relations);
    }
}

fn collect_cte(cte: &CommonTableExpression, relations: &mut BTreeSet<String>) {
    match &cte.query {
        CteQuery::Simple(statement) => collect_parsed_statement(statement, relations),
        CteQuery::Recursive {
            base, recursive, ..
        } => {
            collect_parsed_statement(base, relations);
            collect_parsed_statement(recursive, relations);
        }
    }
    if let Some(cycle) = &cte.cycle {
        collect_expression(&cycle.mark_value, relations);
        collect_expression(&cycle.default_value, relations);
    }
}

fn collect_parsed_statement(statement: &ParsedStatement, relations: &mut BTreeSet<String>) {
    if let QueryStatement::Select(select) = &statement.statement {
        collect_select(select, relations);
    }
}

fn collect_source(source: &QuerySource, relations: &mut BTreeSet<String>) {
    match source {
        QuerySource::Collection(name) => {
            relations.insert(name.to_string());
        }
        QuerySource::Values { rows } => {
            for expression in rows.iter().flatten() {
                collect_expression(expression, relations);
            }
        }
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            collect_source(source, relations);
            collect_expression(rate, relations);
            if let Some(repeatable) = repeatable {
                collect_expression(repeatable, relations);
            }
        }
        QuerySource::Aliased { source, .. } => collect_source(source, relations),
        QuerySource::TableFunction { function, .. } => {
            for expression in function.expressions() {
                collect_expression(expression, relations);
            }
        }
        QuerySource::Subquery { select, .. } => collect_select(select, relations),
        QuerySource::Join {
            left, right, on, ..
        } => {
            collect_source(left, relations);
            collect_source(right, relations);
            collect_expression(on, relations);
        }
        QuerySource::Cte(_) | QuerySource::SingleRow => {}
    }
}

fn collect_item(item: &SelectItem, relations: &mut BTreeSet<String>) {
    match item {
        SelectItem::Expr { expr, .. } => collect_expression(expr, relations),
        SelectItem::Function { function, .. } => {
            for expression in function.expressions() {
                collect_expression(expression, relations);
            }
        }
        SelectItem::WindowFunction { function, .. } => {
            for expression in function
                .args
                .iter()
                .chain(function.filter.as_deref())
                .chain(&function.partition_by)
                .chain(function.order_by.iter().map(|order| &order.expr))
            {
                collect_expression(expression, relations);
            }
            for order in &function.order_by {
                collect_expression(&order.expr, relations);
            }
        }
        SelectItem::Wildcard | SelectItem::Column { .. } => {}
    }
}

fn collect_expression(expression: &Expr, relations: &mut BTreeSet<String>) {
    expression.any_descendant_or_self(&mut |candidate| {
        match candidate {
            Expr::Exists(statement) => collect_parsed_statement(statement, relations),
            Expr::Subquery(subquery) => {
                collect_parsed_statement(&subquery.query, relations);
            }
            _ => {}
        }
        false
    });
}
