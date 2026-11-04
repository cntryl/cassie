//! Bind generated-source and nested array input identities in the concrete OID pass.
use super::coalesce_results::ResultTypes;
use super::{
    BindingContext, CassieError, Catalog, Expr, HashMap, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};
use crate::sql::{
    ast::{CastIdentity, CteQuery, FunctionCall, ParsedStatement},
    table_sources,
};
use crate::types::{DataType, Schema};

pub(crate) fn coerce_statement(
    statement: &mut ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
) -> Result<(), CassieError> {
    statement_with_outer(
        statement,
        catalog,
        context,
        parameters,
        &HashMap::new(),
        None,
    )
}

fn statement_with_outer(
    statement: &mut ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
    ctes: &HashMap<String, Schema>,
    outer: Option<&ResultTypes>,
) -> Result<(), CassieError> {
    match &mut statement.statement {
        QueryStatement::Select(select) => {
            select_inputs(select, catalog, context, parameters, ctes, outer)
        }
        QueryStatement::Explain(explain) => statement_with_outer(
            &mut explain.statement,
            catalog,
            context,
            parameters,
            ctes,
            outer,
        ),
        QueryStatement::Update(update) => {
            let types = ResultTypes::for_source_with_parameters(
                &QuerySource::Collection(update.table.clone()),
                &[],
                catalog,
                context,
                parameters,
            )?;
            for (_, expr) in &mut update.assignments {
                expression(expr, &types, catalog, context, parameters)?;
            }
            if let Some(expr) = &mut update.filter {
                expression(expr, &types, catalog, context, parameters)?;
            }
            items(&mut update.returning, &types, catalog, context, parameters)
        }
        QueryStatement::Delete(delete) => {
            let types = ResultTypes::for_source_with_parameters(
                &QuerySource::Collection(delete.table.clone()),
                &[],
                catalog,
                context,
                parameters,
            )?;
            if let Some(expr) = &mut delete.filter {
                expression(expr, &types, catalog, context, parameters)?;
            }
            items(&mut delete.returning, &types, catalog, context, parameters)
        }
        QueryStatement::Insert(insert) => {
            let types = ResultTypes::for_source_with_parameters(
                &QuerySource::Collection(insert.table.clone()),
                &[],
                catalog,
                context,
                parameters,
            )?;
            match &mut insert.source {
                super::InsertSource::Values(rows) => {
                    for expr in rows.iter_mut().flatten() {
                        expression(expr, &types, catalog, context, parameters)?;
                    }
                }
                super::InsertSource::Select(select) => {
                    select_inputs(select, catalog, context, parameters, ctes, outer)?;
                }
            }
            if let Some(conflict) = &mut insert.on_conflict {
                if let crate::sql::ast::InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                    ..
                } = &mut conflict.action
                {
                    let schema = catalog
                        .get_schema(&insert.table)
                        .ok_or_else(|| CassieError::CollectionNotFound(insert.table.to_string()))?;
                    let conflict_types = ResultTypes::for_source_with_parameters(
                        &QuerySource::Collection(insert.table.clone()),
                        &[],
                        catalog,
                        context,
                        parameters,
                    )?
                    .with_excluded_fields(&schema);
                    for (_, expr) in assignments {
                        expression(expr, &conflict_types, catalog, context, parameters)?;
                    }
                    if let Some(expr) = filter {
                        expression(expr, &conflict_types, catalog, context, parameters)?;
                    }
                }
            }
            items(&mut insert.returning, &types, catalog, context, parameters)
        }
        _ => Ok(()),
    }
}

fn select_inputs(
    select: &mut SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
    ctes: &HashMap<String, Schema>,
    outer: Option<&ResultTypes>,
) -> Result<(), CassieError> {
    if !needed(select) {
        return Ok(());
    }
    let mut scoped = select.clone();
    let names = ctes.keys().cloned().collect();
    super::boolean_contexts::resolve_nested_sources(&mut scoped, &names, catalog, context)?;
    let types = ResultTypes::for_scope_with_parameters(
        &scoped.source,
        &scoped.ctes,
        catalog,
        context,
        ctes,
        parameters,
    )?;
    let types = if let Some(outer) = outer {
        types.with_outer_fields(outer)
    } else {
        types
    };
    for cte in &mut select.ctes {
        match &mut cte.query {
            CteQuery::Simple(statement) => statement_with_outer(
                statement,
                catalog,
                context,
                parameters,
                types.cte_schemas(),
                None,
            )?,
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                statement_with_outer(
                    base,
                    catalog,
                    context,
                    parameters,
                    types.cte_schemas(),
                    None,
                )?;
                statement_with_outer(
                    recursive,
                    catalog,
                    context,
                    parameters,
                    types.cte_schemas(),
                    None,
                )?;
            }
        }
    }
    source(&mut select.source, &types, catalog, context, parameters)?;
    items(&mut select.projection, &types, catalog, context, parameters)?;
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
        .chain(select.order.iter_mut().map(|order| &mut order.expr))
    {
        expression(expr, &types, catalog, context, parameters)?;
    }
    if let Some(set) = &mut select.set {
        select_inputs(
            &mut set.right,
            catalog,
            context,
            parameters,
            types.cte_schemas(),
            None,
        )?;
    }
    Ok(())
}

fn items(
    items: &mut [SelectItem],
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
) -> Result<(), CassieError> {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => expression(expr, types, catalog, context, parameters)?,
            SelectItem::Function { function, .. } => {
                function_arguments(function, types, catalog, context, parameters)?;
            }
            SelectItem::WindowFunction { function, .. } => {
                for arg in function
                    .args
                    .iter_mut()
                    .chain(function.filter.as_deref_mut())
                    .chain(&mut function.partition_by)
                    .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                {
                    expression(arg, types, catalog, context, parameters)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn source(
    source: &mut QuerySource,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
) -> Result<(), CassieError> {
    match source {
        QuerySource::Aliased { source: inner, .. }
        | QuerySource::TableSample { source: inner, .. } => {
            self::source(inner, types, catalog, context, parameters)?;
        }
        QuerySource::Join {
            left, right, on, ..
        } => {
            self::source(left, types, catalog, context, parameters)?;
            self::source(right, types, catalog, context, parameters)?;
            expression(on, types, catalog, context, parameters)?;
        }
        QuerySource::Subquery {
            select, lateral, ..
        } => select_inputs(
            select,
            catalog,
            context,
            parameters,
            types.cte_schemas(),
            lateral.then_some(types),
        )?,
        QuerySource::TableFunction { function, .. } => {
            function_arguments(function, types, catalog, context, parameters)?;
            if table_sources::is_generated(&function.name) {
                for argument in &mut function.args {
                    super::boolean_contexts::validate_value(
                        argument, types, None, catalog, context,
                    )?;
                }
            }
            let signature = if table_sources::is_series(&function.name) {
                Some(crate::sql::series_signature::resolve(
                    function,
                    |argument| types.expression_type(argument),
                )?)
            } else {
                None
            };
            for (index, argument) in function.args.iter_mut().enumerate() {
                if let Some(signature) = &signature {
                    cast_input(argument, &signature.inputs[index], types);
                } else if table_sources::is_unnest(&function.name) {
                    if let Some(kind @ DataType::Array(_)) = types.expression_type(argument) {
                        cast_input(argument, &kind, types);
                    }
                }
            }
        }
        QuerySource::Values { rows } => {
            for expr in rows.iter_mut().flatten() {
                expression(expr, types, catalog, context, parameters)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn expression(
    expr: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
) -> Result<(), CassieError> {
    if let Expr::Cast {
        expr: child,
        data_type: target @ DataType::Array(_),
        ..
    } = expr
    {
        if let Expr::Function(function) = child.as_mut() {
            if function
                .name
                .eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION)
            {
                return typed_function_arguments(
                    function,
                    types,
                    catalog,
                    context,
                    parameters,
                    Some(target),
                );
            }
        }
    }
    if let Some(statement) = expr.subquery_statement_mut() {
        statement_with_outer(
            statement,
            catalog,
            context,
            parameters,
            types.cte_schemas(),
            Some(types),
        )?;
    }
    if let Expr::Function(function) = expr {
        return function_arguments(function, types, catalog, context, parameters);
    }
    expr.try_visit_children_mut(|child| expression(child, types, catalog, context, parameters))
}

fn function_arguments(
    function: &mut FunctionCall,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
) -> Result<(), CassieError> {
    typed_function_arguments(function, types, catalog, context, parameters, None)
}

fn typed_function_arguments(
    function: &mut FunctionCall,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
    parameters: &[i32],
    array_type: Option<&DataType>,
) -> Result<(), CassieError> {
    let constructor = function
        .name
        .eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION);
    if constructor && function.args.is_empty() && array_type.is_none() {
        return Err(crate::app::DiagnosticError::new(
            "42P18",
            "cannot determine type of empty array",
        )
        .into());
    }
    let mut common = DataType::Null;
    if constructor {
        for argument in &function.args {
            if matches!(argument, Expr::Null | Expr::StringLiteral(_)) {
                continue;
            }
            if let Some(kind) = types.expression_type(argument) {
                common = super::common_case_type(common, kind).ok_or_else(|| {
                    crate::app::DiagnosticError::new("42804", "ARRAY types cannot be matched")
                })?;
            }
        }
        if common == DataType::Null {
            common = DataType::Text;
        }
    }
    for argument in function.expressions_mut() {
        if constructor && array_type.is_some() {
            if let Expr::Function(child) = argument {
                if child
                    .name
                    .eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION)
                {
                    typed_function_arguments(
                        child, types, catalog, context, parameters, array_type,
                    )?;
                    continue;
                }
            }
        }
        expression(argument, types, catalog, context, parameters)?;
        if constructor {
            cast_input(argument, &common, types);
        }
    }
    Ok(())
}

fn cast_input(expr: &mut Expr, target: &DataType, types: &ResultTypes) {
    if matches!(expr,Expr::Cast { data_type,.. } if data_type == target) {
        return;
    }
    let source_type = types.expression_type(expr);
    let original = std::mem::replace(expr, Expr::Null);
    *expr = Expr::Cast {
        expr: Box::new(original),
        data_type: target.clone(),
        source_type,
        identity: CastIdentity::Scalar,
    };
}

fn expression_needed(expr: &Expr) -> bool {
    expr.any_descendant_or_self(&mut|expr| {
        matches!(expr,Expr::Function(function) if function.name.eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION))
            || matches!(expr,Expr::Subquery(subquery) if matches!(subquery.kind,crate::sql::ast::SubqueryKind::Array))
            || expr.subquery_statement().is_some_and(statement_needed)
    })
}
fn statement_needed(statement: &ParsedStatement) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => needed(select),
        QueryStatement::Explain(explain) => statement_needed(&explain.statement),
        _ => false,
    }
}
fn needed(select: &SelectStatement) -> bool {
    source_needed(&select.source)
        || select.ctes.iter().any(|cte| match &cte.query {
            CteQuery::Simple(statement) => statement_needed(statement),
            CteQuery::Recursive {
                base, recursive, ..
            } => statement_needed(base) || statement_needed(recursive),
        })
        || select.set.as_ref().is_some_and(|set| needed(&set.right))
        || select.projection.iter().any(|item| match item {
            SelectItem::Expr { expr, .. } => expression_needed(expr),
            SelectItem::Function { function, .. } => {
                function
                    .name
                    .eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION)
                    || function.expressions().any(expression_needed)
            }
            SelectItem::WindowFunction { function, .. } => function
                .args
                .iter()
                .chain(function.filter.as_deref())
                .chain(&function.partition_by)
                .chain(function.order_by.iter().map(|order| &order.expr))
                .any(expression_needed),
            _ => false,
        })
        || select
            .filter
            .iter()
            .chain(&select.having)
            .chain(&select.group_by)
            .chain(&select.distinct_on)
            .chain(select.order.iter().map(|order| &order.expr))
            .any(expression_needed)
}
fn source_needed(source: &QuerySource) -> bool {
    match source {
        QuerySource::TableFunction { function, .. } => table_sources::is_generated(&function.name),
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            source_needed(source)
        }
        QuerySource::Join { left, right, .. } => source_needed(left) || source_needed(right),
        QuerySource::Subquery { select, .. } => needed(select),
        QuerySource::Values { rows } => rows.iter().flatten().any(expression_needed),
        _ => false,
    }
}
