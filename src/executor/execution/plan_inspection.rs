use std::collections::HashSet;

use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, FunctionCall, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};

pub(super) fn fulltext_query_fields(plan: &LogicalPlan) -> HashSet<String> {
    let mut fields = HashSet::new();

    if let Some(filter) = &plan.filter {
        collect_fulltext_fields_from_expr(filter, &mut fields);
    }

    for order in &plan.order {
        collect_fulltext_fields_from_expr(&order.expr, &mut fields);
    }

    for item in &plan.projection {
        collect_fulltext_fields_from_select_item(item, &mut fields);
    }

    fields
}

pub(super) fn plan_uses_function(plan: &LogicalPlan, function_name: &str) -> bool {
    if let Some(filter) = &plan.filter {
        if expr_uses_function(filter, function_name) {
            return true;
        }
    }

    if plan
        .order
        .iter()
        .any(|order| expr_uses_function(&order.expr, function_name))
    {
        return true;
    }

    if plan
        .projection
        .iter()
        .any(|item| select_item_uses_function(item, function_name))
    {
        return true;
    }

    plan.ctes
        .iter()
        .any(|cte| cte_uses_function(cte, function_name))
}

pub(crate) fn plan_needs_user_functions(plan: &LogicalPlan) -> bool {
    query_source_needs_user_functions(&plan.source)
        || plan.projection.iter().any(select_item_needs_user_functions)
        || plan.filter.as_ref().is_some_and(expr_needs_user_functions)
        || plan.distinct_on.iter().any(expr_needs_user_functions)
        || plan.group_by.iter().any(expr_needs_user_functions)
        || plan.having.as_ref().is_some_and(expr_needs_user_functions)
        || plan
            .order
            .iter()
            .any(|order| expr_needs_user_functions(&order.expr))
        || plan.ctes.iter().any(cte_needs_user_functions)
        || plan
            .set
            .as_ref()
            .is_some_and(|set| select_needs_user_functions(&set.right))
}

fn cte_needs_user_functions(cte: &CommonTableExpression) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_needs_user_functions(statement),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_needs_user_functions(base)
                || parsed_statement_needs_user_functions(recursive)
        }
    }
}

fn parsed_statement_needs_user_functions(statement: &crate::sql::ast::ParsedStatement) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => select_needs_user_functions(select),
        _ => false,
    }
}

fn select_needs_user_functions(select: &SelectStatement) -> bool {
    query_source_needs_user_functions(&select.source)
        || select
            .projection
            .iter()
            .any(select_item_needs_user_functions)
        || select
            .filter
            .as_ref()
            .is_some_and(expr_needs_user_functions)
        || select.distinct_on.iter().any(expr_needs_user_functions)
        || select.group_by.iter().any(expr_needs_user_functions)
        || select
            .having
            .as_ref()
            .is_some_and(expr_needs_user_functions)
        || select
            .order
            .iter()
            .any(|order| expr_needs_user_functions(&order.expr))
        || select.ctes.iter().any(cte_needs_user_functions)
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_needs_user_functions(&set.right))
}

fn query_source_needs_user_functions(source: &QuerySource) -> bool {
    match source {
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
        QuerySource::TableFunction { function, .. } => {
            function.args.iter().any(expr_needs_user_functions)
        }
        QuerySource::Subquery { select, .. } => select_needs_user_functions(select),
        QuerySource::Join {
            left, right, on, ..
        } => {
            query_source_needs_user_functions(left)
                || query_source_needs_user_functions(right)
                || expr_needs_user_functions(on)
        }
    }
}

fn select_item_needs_user_functions(item: &SelectItem) -> bool {
    match item {
        SelectItem::Function { function, .. } => function_needs_user_functions(function),
        SelectItem::Expr { expr, .. } => expr_needs_user_functions(expr),
        SelectItem::Column { .. } | SelectItem::Wildcard | SelectItem::WindowFunction { .. } => {
            false
        }
    }
}

fn expr_needs_user_functions(expr: &Expr) -> bool {
    match expr {
        Expr::Binary { left, right, .. } => {
            expr_needs_user_functions(left) || expr_needs_user_functions(right)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => expr_needs_user_functions(expr),
        Expr::InList { expr, values, .. } => {
            expr_needs_user_functions(expr) || values.iter().any(expr_needs_user_functions)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_needs_user_functions(expr)
                || expr_needs_user_functions(low)
                || expr_needs_user_functions(high)
        }
        Expr::Not { expr } => expr_needs_user_functions(expr),
        Expr::Exists(statement) => parsed_statement_needs_user_functions(statement),
        Expr::Function(function) => function_needs_user_functions(function),
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => false,
    }
}

fn function_needs_user_functions(function: &FunctionCall) -> bool {
    let name = function.name.to_ascii_lowercase();
    let is_builtin = matches!(
        name.as_str(),
        "search"
            | "search_score"
            | "vector_distance"
            | "vector_score"
            | "cosine_distance"
            | "dot_product"
            | "hybrid_score"
            | "snippet"
            | "version"
            | "current_schema"
            | "current_database"
            | "current_user"
            | "session_user"
            | "current_role"
            | "quote_ident"
            | "pg_catalog.quote_ident"
            | "format_type"
            | "pg_catalog.format_type"
            | "pg_get_expr"
            | "pg_catalog.pg_get_expr"
            | "pg_get_userbyid"
            | "pg_catalog.pg_get_userbyid"
            | "obj_description"
            | "pg_catalog.obj_description"
            | "has_schema_privilege"
            | "pg_catalog.has_schema_privilege"
            | "has_table_privilege"
            | "pg_catalog.has_table_privilege"
            | "pg_table_is_visible"
            | "pg_catalog.pg_table_is_visible"
            | "length"
            | "len"
            | "lower"
            | "upper"
            | "substring"
            | "trim"
            | "concat"
            | "coalesce"
            | "abs"
            | "time_bucket"
            | "cast"
    ) || crate::sql::functions::is_aggregate_function(&function.name);

    !is_builtin || function.args.iter().any(expr_needs_user_functions)
}

fn cte_uses_function(cte: &CommonTableExpression, function_name: &str) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_uses_function(statement, function_name),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_uses_function(base, function_name)
                || parsed_statement_uses_function(recursive, function_name)
        }
    }
}

pub(super) fn plan_uses_vector_operator(plan: &LogicalPlan) -> bool {
    if let Some(filter) = &plan.filter {
        if expr_uses_vector_operator(filter) {
            return true;
        }
    }

    if plan
        .order
        .iter()
        .any(|order| expr_uses_vector_operator(&order.expr))
    {
        return true;
    }

    if plan.projection.iter().any(select_item_uses_vector_operator) {
        return true;
    }

    plan.ctes.iter().any(cte_uses_vector_operator)
}

fn cte_uses_vector_operator(cte: &CommonTableExpression) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_uses_vector_operator(statement),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_uses_vector_operator(base)
                || parsed_statement_uses_vector_operator(recursive)
        }
    }
}

fn function_uses_vector_operator(function: &crate::sql::ast::FunctionCall) -> bool {
    if function.name.eq_ignore_ascii_case("vector_distance")
        || function.name.eq_ignore_ascii_case("cosine_distance")
        || function.name.eq_ignore_ascii_case("dot_product")
        || function.name.eq_ignore_ascii_case("vector_score")
    {
        true
    } else {
        function.args.iter().any(expr_uses_vector_operator)
    }
}

fn select_item_uses_vector_operator(item: &crate::sql::ast::SelectItem) -> bool {
    match item {
        crate::sql::ast::SelectItem::Function { function, .. } => {
            function_uses_vector_operator(function)
        }
        _ => false,
    }
}

fn expr_uses_vector_operator(expr: &crate::sql::ast::Expr) -> bool {
    match expr {
        crate::sql::ast::Expr::Binary {
            left, right, op, ..
        } => {
            matches!(
                op,
                crate::sql::ast::BinaryOp::PgvectorCosine
                    | crate::sql::ast::BinaryOp::PgvectorL2
                    | crate::sql::ast::BinaryOp::PgvectorDot
            ) || expr_uses_vector_operator(left)
                || expr_uses_vector_operator(right)
        }
        crate::sql::ast::Expr::Function(function) => function_uses_vector_operator(function),
        crate::sql::ast::Expr::IsNull { expr, .. } => expr_uses_vector_operator(expr),
        crate::sql::ast::Expr::InList { expr, values, .. } => {
            expr_uses_vector_operator(expr) || values.iter().any(expr_uses_vector_operator)
        }
        crate::sql::ast::Expr::Between {
            expr, low, high, ..
        } => {
            expr_uses_vector_operator(expr)
                || expr_uses_vector_operator(low)
                || expr_uses_vector_operator(high)
        }
        crate::sql::ast::Expr::Cast { expr, .. } => expr_uses_vector_operator(expr),
        _ => false,
    }
}

fn parsed_statement_uses_vector_operator(statement: &crate::sql::ast::ParsedStatement) -> bool {
    match &statement.statement {
        crate::sql::ast::QueryStatement::Select(select) => select_uses_vector_operator(select),
        _ => false,
    }
}

fn select_uses_vector_operator(select: &crate::sql::ast::SelectStatement) -> bool {
    select
        .filter
        .as_ref()
        .is_some_and(expr_uses_vector_operator)
        || select
            .order
            .iter()
            .any(|order| expr_uses_vector_operator(&order.expr))
        || select.ctes.iter().any(cte_uses_vector_operator)
}

fn parsed_statement_uses_function(
    statement: &crate::sql::ast::ParsedStatement,
    function_name: &str,
) -> bool {
    match &statement.statement {
        crate::sql::ast::QueryStatement::Select(select) => {
            select_uses_function(select, function_name)
        }
        _ => false,
    }
}

fn select_uses_function(select: &crate::sql::ast::SelectStatement, function_name: &str) -> bool {
    select
        .projection
        .iter()
        .any(|item| select_item_uses_function(item, function_name))
        || select
            .filter
            .as_ref()
            .is_some_and(|expr| expr_uses_function(expr, function_name))
        || select
            .order
            .iter()
            .any(|order| expr_uses_function(&order.expr, function_name))
        || select
            .ctes
            .iter()
            .any(|cte| cte_uses_function(cte, function_name))
}

fn select_item_uses_function(item: &crate::sql::ast::SelectItem, function_name: &str) -> bool {
    match item {
        crate::sql::ast::SelectItem::Function { function, .. } => {
            function_uses_function(function, function_name)
        }
        _ => false,
    }
}

fn expr_uses_function(expr: &crate::sql::ast::Expr, function_name: &str) -> bool {
    match expr {
        crate::sql::ast::Expr::Binary { left, right, .. } => {
            expr_uses_function(left, function_name) || expr_uses_function(right, function_name)
        }
        crate::sql::ast::Expr::Function(function) => {
            function_uses_function(function, function_name)
        }
        crate::sql::ast::Expr::IsNull { expr, .. } => expr_uses_function(expr, function_name),
        crate::sql::ast::Expr::InList { expr, values, .. } => {
            expr_uses_function(expr, function_name)
                || values
                    .iter()
                    .any(|value| expr_uses_function(value, function_name))
        }
        crate::sql::ast::Expr::Between {
            expr, low, high, ..
        } => {
            expr_uses_function(expr, function_name)
                || expr_uses_function(low, function_name)
                || expr_uses_function(high, function_name)
        }
        crate::sql::ast::Expr::Cast { expr, .. } => expr_uses_function(expr, function_name),
        _ => false,
    }
}

fn function_uses_function(function: &crate::sql::ast::FunctionCall, function_name: &str) -> bool {
    function.name.eq_ignore_ascii_case(function_name)
        || function
            .args
            .iter()
            .any(|expr| expr_uses_function(expr, function_name))
}

fn collect_fulltext_fields_from_select_item(
    item: &crate::sql::ast::SelectItem,
    fields: &mut HashSet<String>,
) {
    if let crate::sql::ast::SelectItem::Function { function, .. } = item {
        collect_fulltext_fields_from_function(function, fields);
    }
}

fn collect_fulltext_fields_from_expr(expr: &crate::sql::ast::Expr, fields: &mut HashSet<String>) {
    match expr {
        crate::sql::ast::Expr::Binary { left, right, .. } => {
            collect_fulltext_fields_from_expr(left, fields);
            collect_fulltext_fields_from_expr(right, fields);
        }
        crate::sql::ast::Expr::Function(function) => {
            collect_fulltext_fields_from_function(function, fields);
        }
        crate::sql::ast::Expr::IsNull { expr, .. } => {
            collect_fulltext_fields_from_expr(expr, fields);
        }
        crate::sql::ast::Expr::InList { expr, values, .. } => {
            collect_fulltext_fields_from_expr(expr, fields);
            for value in values {
                collect_fulltext_fields_from_expr(value, fields);
            }
        }
        crate::sql::ast::Expr::Between {
            expr, low, high, ..
        } => {
            collect_fulltext_fields_from_expr(expr, fields);
            collect_fulltext_fields_from_expr(low, fields);
            collect_fulltext_fields_from_expr(high, fields);
        }
        crate::sql::ast::Expr::Cast { expr, .. } => {
            collect_fulltext_fields_from_expr(expr, fields);
        }
        _ => {}
    }
}

fn collect_fulltext_fields_from_function(
    function: &crate::sql::ast::FunctionCall,
    fields: &mut HashSet<String>,
) {
    let name = function.name.to_ascii_lowercase();
    if matches!(name.as_str(), "search" | "search_score") {
        if let Some(crate::sql::ast::Expr::Column(field)) = function.args.first() {
            fields.insert(field.to_ascii_lowercase());
        }
    }

    for arg in &function.args {
        collect_fulltext_fields_from_expr(arg, fields);
    }
}

pub(super) fn logical_plan_from_select(select: &SelectStatement) -> LogicalPlan {
    LogicalPlan {
        command: None,
        source: select.source.clone(),
        collection: execution_source_name(&select.source),
        ctes: select.ctes.clone(),
        distinct: select.distinct,
        distinct_on: select.distinct_on.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        group_by: select.group_by.clone(),
        having: select.having.clone(),
        order: select.order.clone(),
        limit: select.limit,
        offset: select.offset,
        set: select.set.clone(),
    }
}

fn execution_source_name(source: &QuerySource) -> String {
    match source {
        QuerySource::Collection(name) | QuerySource::Cte(name) => name.clone(),
        QuerySource::TableFunction { name, .. } => name.clone(),
        QuerySource::Subquery { alias, .. } => alias.clone(),
        QuerySource::SingleRow => "single_row".to_string(),
        QuerySource::Join { .. } => "join".to_string(),
    }
}
