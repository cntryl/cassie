use super::coalesce_results::ResultTypes;
use super::{
    BinaryOp, BindingContext, CassieError, Catalog, DataType, Expr, QuerySource, SelectItem,
    SelectStatement,
};

pub(super) fn validate_select(
    select: &mut SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let types = ResultTypes::for_source(&select.source, &select.ctes, catalog, context)?;
    validate_select_with_types(select, &types, catalog, context)
}

pub(super) fn validate_select_with_types(
    select: &mut SelectStatement,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if let Some(filter) = &mut select.filter {
        require_boolean(filter, types, "WHERE")?;
    }
    if let Some(having) = &mut select.having {
        require_boolean(having, types, "HAVING")?;
    }
    validate_source_predicates(&mut select.source, types, catalog, context)?;
    for item in &mut select.projection {
        match item {
            SelectItem::Expr { expr, .. } => validate_expression(expr, types, catalog, context)?,
            SelectItem::Function { function, .. } => {
                for argument in &mut function.args {
                    validate_expression(argument, types, catalog, context)?;
                }
                super::coalesce_coercion::coerce_function(function, types);
            }
            SelectItem::WindowFunction { function, .. } => {
                for expression in function
                    .args
                    .iter_mut()
                    .chain(&mut function.partition_by)
                    .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                {
                    validate_expression(expression, types, catalog, context)?;
                }
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
        super::conditional_types::validate_item(item, types)?;
    }
    for expression in select
        .filter
        .iter_mut()
        .chain(&mut select.distinct_on)
        .chain(&mut select.group_by)
        .chain(&mut select.having)
        .chain(select.order.iter_mut().map(|order| &mut order.expr))
    {
        validate_expression(expression, types, catalog, context)?;
        super::conditional_types::validate_expression(expression, types)?;
    }
    for cte in &mut select.ctes {
        match &mut cte.query {
            super::CteQuery::Simple(statement) => {
                validate_nested_statement(statement, types, false, catalog, context)?;
            }
            super::CteQuery::Recursive {
                base, recursive, ..
            } => {
                validate_nested_statement(base, types, false, catalog, context)?;
                validate_nested_statement(recursive, types, false, catalog, context)?;
            }
        }
    }
    if let Some(set) = &mut select.set {
        let nested = ResultTypes::for_scope_with_parameters(
            &set.right.source,
            &set.right.ctes,
            catalog,
            context,
            types.cte_schemas(),
            types.parameter_types(),
        )?
        .with_resolved_coalesce_domains(types.resolved_coalesce_domains());
        validate_select_with_types(&mut set.right, &nested, catalog, context)?;
    }
    Ok(())
}

fn validate_source_predicates(
    source: &mut QuerySource,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Aliased { source, .. } => {
            validate_source_predicates(source, types, catalog, context)?;
        }
        QuerySource::Join {
            left, right, on, ..
        } => {
            require_boolean(on, types, "JOIN ON")?;
            validate_expression(on, types, catalog, context)?;
            super::conditional_types::validate_expression(on, types)?;
            validate_source_predicates(left, types, catalog, context)?;
            validate_source_predicates(right, types, catalog, context)?;
        }
        QuerySource::Subquery {
            select, lateral, ..
        } => {
            let inner = ResultTypes::for_scope_with_parameters(
                &select.source,
                &select.ctes,
                catalog,
                context,
                types.cte_schemas(),
                types.parameter_types(),
            )?
            .with_resolved_coalesce_domains(types.resolved_coalesce_domains());
            let inner = if *lateral {
                inner.with_outer_fields(types)
            } else {
                inner
            };
            validate_select_with_types(select, &inner, catalog, context)?;
        }
        QuerySource::Collection(_)
        | QuerySource::Cte(_)
        | QuerySource::SingleRow
        | QuerySource::TableFunction { .. } => {}
    }
    Ok(())
}

fn require_boolean(expr: &mut Expr, types: &ResultTypes, context: &str) -> Result<(), CassieError> {
    if let Expr::StringLiteral(text) = expr {
        match crate::types::boolean::parse_text(text) {
            Some(value) => *expr = Expr::BoolLiteral(value),
            None => {
                return Err(CassieError::Planner(format!(
                    "invalid input syntax for type boolean: {text:?}"
                )));
            }
        }
    }
    match types.expression_type(expr) {
        Some(DataType::Boolean | DataType::Null) | None => Ok(()),
        Some(data_type) => Err(CassieError::Planner(format!(
            "{context} expression must have BOOLEAN type (found {data_type:?})"
        ))),
    }
}

pub(super) fn validate_predicate(
    expr: &mut Expr,
    types: &ResultTypes,
    label: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    require_boolean(expr, types, label)?;
    validate_expression(expr, types, catalog, context)?;
    super::conditional_types::validate_expression(expr, types)?;
    Ok(())
}

pub(super) fn validate_value(
    expr: &mut Expr,
    types: &ResultTypes,
    expected: Option<&DataType>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if expected == Some(&DataType::Boolean) {
        require_boolean(expr, types, "Boolean assignment")?;
    }
    validate_expression(expr, types, catalog, context)?;
    super::conditional_types::validate_expression(expr, types)?;
    Ok(())
}

fn validate_expression(
    expr: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And | BinaryOp::Or,
            right,
        } => {
            require_boolean(left, types, "Boolean operator")?;
            require_boolean(right, types, "Boolean operator")?;
            validate_expression(left, types, catalog, context)?;
            validate_expression(right, types, catalog, context)?;
        }
        Expr::Binary { left, op, right } => {
            if matches!(
                op,
                BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::Lte
                    | BinaryOp::Gt
                    | BinaryOp::Gte
            ) {
                canonicalize_boolean_pair(left, right, types)?;
            }
            validate_expression(left, types, catalog, context)?;
            validate_expression(right, types, catalog, context)?;
        }
        Expr::Not { expr } => {
            require_boolean(expr, types, "NOT")?;
            validate_expression(expr, types, catalog, context)?;
        }
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            let searched = operand.is_none();
            if let Some(operand) = operand {
                validate_expression(operand, types, catalog, context)?;
            }
            for (condition, result) in branches {
                if searched {
                    require_boolean(condition, types, "CASE WHEN")?;
                } else if let Some(operand) = operand {
                    canonicalize_boolean_pair(operand, condition, types)?;
                }
                validate_expression(condition, types, catalog, context)?;
                validate_expression(result, types, catalog, context)?;
            }
            if let Some(result) = else_expr {
                validate_expression(result, types, catalog, context)?;
            }
        }
        Expr::Cast { expr, .. } | Expr::IsNull { expr, .. } => {
            validate_expression(expr, types, catalog, context)?;
        }
        Expr::InList { expr, values, .. } => {
            validate_expression(expr, types, catalog, context)?;
            for value in values {
                canonicalize_boolean_pair(expr, value, types)?;
                validate_expression(value, types, catalog, context)?;
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            canonicalize_boolean_pair(expr, low, types)?;
            canonicalize_boolean_pair(expr, high, types)?;
            canonicalize_boolean_pair(low, high, types)?;
            validate_expression(expr, types, catalog, context)?;
            validate_expression(low, types, catalog, context)?;
            validate_expression(high, types, catalog, context)?;
        }
        Expr::Function(function) => {
            for argument in &mut function.args {
                validate_expression(argument, types, catalog, context)?;
            }
            super::coalesce_coercion::coerce_function(function, types);
        }
        Expr::Exists(statement) => {
            validate_nested_statement(statement, types, true, catalog, context)?;
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => {}
    }
    Ok(())
}

fn canonicalize_boolean_pair(
    left: &mut Expr,
    right: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if types.expression_type(left) == Some(DataType::Boolean) {
        require_boolean(right, types, "Boolean comparison")?;
    }
    if types.expression_type(right) == Some(DataType::Boolean) {
        require_boolean(left, types, "Boolean comparison")?;
    }
    Ok(())
}

fn validate_nested_statement(
    statement: &mut super::ParsedStatement,
    outer: &ResultTypes,
    correlated: bool,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if let super::QueryStatement::Select(select) = &mut statement.statement {
        let names = outer.cte_schemas().keys().cloned().collect();
        let mut scoped = select.clone();
        resolve_nested_sources(&mut scoped, &names, catalog, context)?;
        let mut types = ResultTypes::for_scope_with_parameters(
            &scoped.source,
            &scoped.ctes,
            catalog,
            context,
            outer.cte_schemas(),
            outer.parameter_types(),
        )?
        .with_resolved_coalesce_domains(outer.resolved_coalesce_domains());
        if correlated {
            types = types.with_outer_fields(outer);
        }
        validate_select_with_types(select, &types, catalog, context)?;
    }
    Ok(())
}

pub(super) fn resolve_nested_sources(
    select: &mut SelectStatement,
    outer: &super::HashSet<String>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let mut names = outer.clone();
    for cte in &mut select.ctes {
        if matches!(cte.query, super::CteQuery::Recursive { .. }) {
            names.insert(cte.name.to_ascii_lowercase());
        }
        match &mut cte.query {
            super::CteQuery::Simple(statement) => {
                resolve_nested_statement_sources(statement, &names, catalog, context)?;
            }
            super::CteQuery::Recursive {
                base, recursive, ..
            } => {
                resolve_nested_statement_sources(base, &names, catalog, context)?;
                resolve_nested_statement_sources(recursive, &names, catalog, context)?;
            }
        }
        names.insert(cte.name.to_ascii_lowercase());
    }
    resolve_nested_source(&mut select.source, &names, catalog, context)?;
    if let Some(set) = &mut select.set {
        resolve_nested_sources(&mut set.right, &names, catalog, context)?;
    }
    Ok(())
}

fn resolve_nested_statement_sources(
    statement: &mut super::ParsedStatement,
    names: &super::HashSet<String>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if let super::QueryStatement::Select(select) = &mut statement.statement {
        resolve_nested_sources(select, names, catalog, context)?;
    }
    Ok(())
}

fn resolve_nested_source(
    source: &mut QuerySource,
    names: &super::HashSet<String>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Aliased { source, .. } => {
            resolve_nested_source(source, names, catalog, context)?;
        }
        QuerySource::Collection(name) => {
            if names.contains(&name.to_ascii_lowercase()) {
                *source = QuerySource::Cte(name.to_string());
            } else if !(name.components().len() == 3
                && name.components()[0]
                    .value()
                    .eq_ignore_ascii_case(&context.database)
                && catalog.relation_exists(name))
            {
                let resolved = super::resolve_relation_path(name, catalog, context)?;
                *name = crate::sql::ast::IdentifierPath::parse(&resolved)
                    .map_err(CassieError::Planner)?;
            }
        }
        QuerySource::Subquery { select, .. } => {
            resolve_nested_sources(select, names, catalog, context)?;
        }
        QuerySource::Join { left, right, .. } => {
            resolve_nested_source(left, names, catalog, context)?;
            resolve_nested_source(right, names, catalog, context)?;
        }
        QuerySource::Cte(_) | QuerySource::SingleRow | QuerySource::TableFunction { .. } => {}
    }
    Ok(())
}
