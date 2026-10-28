//! Native source/projection reference order and controlled literal admission.
use super::{CassieError, Expr, FunctionCall, HashSet, SelectItem};
use crate::runtime::QueryExecutionControls;

#[derive(Clone, Copy, Default)]
pub(super) struct ReferenceScope<'a, 'c> {
    controls: Option<&'c QueryExecutionControls>,
    nested: Option<NestedScope<'a, 'c>>,
}

#[derive(Clone, Copy)]
struct NestedScope<'a, 'c> {
    ctes: &'a super::CteScope<'c>,
    catalog: &'a super::Catalog,
    context: &'a super::BindingContext,
}

impl<'a, 'c> ReferenceScope<'a, 'c> {
    pub(super) fn new(
        ctes: &'a super::CteScope<'c>,
        catalog: &'a super::Catalog,
        context: &'a super::BindingContext,
    ) -> Self {
        Self {
            controls: ctes.controls(),
            nested: Some(NestedScope {
                ctes,
                catalog,
                context,
            }),
        }
    }

    fn validate_nested(
        self,
        statement: &super::ParsedStatement,
        outer_fields: &HashSet<String>,
    ) -> Result<(), CassieError> {
        let Some(nested) = self.nested else {
            return Ok(());
        };
        crate::sql::parse_analysis::validate_rejected_copy(statement, self.controls, |copy| {
            if let super::QueryStatement::Select(select) = copy.statement {
                super::select::bind_select_with_lateral_fields(
                    *select,
                    nested.catalog,
                    nested.ctes,
                    outer_fields,
                    nested.context,
                )
                .map(drop)?;
            }
            Ok(())
        })
    }
}

pub(super) fn validate_projection_references(
    projection: &[SelectItem],
    known_fields: &HashSet<String>,
    scope: ReferenceScope<'_, '_>,
) -> Result<(), CassieError> {
    for item in projection {
        match item {
            SelectItem::Wildcard => {}
            SelectItem::Column { name, .. } => {
                validate_expression_in_scope(
                    &Expr::Column(name.clone()),
                    known_fields,
                    &HashSet::new(),
                    false,
                    scope,
                )?;
            }
            SelectItem::Function { function, .. } => {
                if crate::sql::functions::is_aggregate_function(&function.name) {
                    validate_aggregate_function_args(function, known_fields, scope)?;
                    continue;
                }
                for arg in function.expressions() {
                    validate_expression_in_scope(arg, known_fields, &HashSet::new(), false, scope)?;
                }
            }
            SelectItem::Expr { expr, .. } => {
                validate_expression_in_scope(expr, known_fields, &HashSet::new(), false, scope)?;
            }
            SelectItem::WindowFunction { function, .. } => {
                for arg in &function.args {
                    validate_expression_in_scope(arg, known_fields, &HashSet::new(), false, scope)?;
                }
                for expr in &function.partition_by {
                    validate_expression_in_scope(
                        expr,
                        known_fields,
                        &HashSet::new(),
                        false,
                        scope,
                    )?;
                }
                for order in &function.order_by {
                    validate_expression_in_scope(
                        &order.expr,
                        known_fields,
                        &HashSet::new(),
                        false,
                        scope,
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn validate_expression_references(
    expression: Option<&Expr>,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
    scope: ReferenceScope<'_, '_>,
) -> Result<(), CassieError> {
    let Some(expression) = expression else {
        return Ok(());
    };
    validate_expression_in_scope(
        expression,
        known_fields,
        projection_aliases,
        allow_projection_alias,
        scope,
    )
}

pub(super) fn validate_order_by_references(
    order: &[crate::sql::ast::OrderExpr],
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    scope: ReferenceScope<'_, '_>,
) -> Result<(), CassieError> {
    for item in order {
        validate_expression_in_scope(&item.expr, known_fields, projection_aliases, true, scope)?;
    }
    Ok(())
}

pub(super) fn validate_column_reference(
    name: &str,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    if known_fields.contains(&format!("__cassie_ambiguous__.{name}")) {
        return Err(CassieError::AmbiguousColumn(name.to_string()));
    }
    if known_fields.contains("*") || known_fields.contains(name) {
        return Ok(());
    }
    if allow_projection_alias
        && projection_aliases.contains(&crate::sql::ColumnIdentifierPath::reference_field_key(name))
    {
        return Ok(());
    }
    Err(CassieError::UndefinedColumn(name.into()))
}

pub(super) fn validate_expression_in_scope(
    expr: &Expr,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
    scope: ReferenceScope<'_, '_>,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => {
            return validate_column_reference(
                name,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            );
        }
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) =>
        {
            return validate_aggregate_function_args(function, known_fields, scope);
        }
        Expr::Exists(statement) => {
            return scope.validate_nested(statement, known_fields);
        }
        Expr::Subquery(subquery) => scope.validate_nested(&subquery.query, known_fields)?,
        _ => {}
    }
    expr.try_visit_children(|child| {
        validate_expression_in_scope(
            child,
            known_fields,
            projection_aliases,
            allow_projection_alias,
            scope,
        )
    })?;
    if let Expr::Cast {
        expr,
        data_type,
        identity,
        ..
    } = expr
    {
        crate::sql::literal_inputs::validate_with_controls(
            expr,
            data_type,
            *identity,
            scope.controls,
        )?;
    }
    Ok(())
}

pub(super) fn validate_aggregate_function_args(
    function: &FunctionCall,
    known_fields: &HashSet<String>,
    scope: ReferenceScope<'_, '_>,
) -> Result<(), CassieError> {
    let Some(metadata) =
        crate::sql::functions::function(&function.name).filter(|metadata| metadata.is_aggregate)
    else {
        return Ok(());
    };
    for arg in &function.args {
        if metadata.id == crate::sql::functions::FunctionId::Count
            && matches!(arg, Expr::Column(name) if name == "*")
        {
            continue;
        }
        validate_expression_in_scope(arg, known_fields, &HashSet::new(), false, scope)?;
    }
    if !metadata.arity.matches(function.args.len()) {
        return Err(CassieError::Planner(format!(
            "aggregate function '{}' expects {}, got {} args",
            function.name,
            metadata.arity.describe(),
            function.args.len()
        )));
    }
    for arg in function
        .filter
        .iter()
        .map(Box::as_ref)
        .chain(function.order_by.iter().map(|order| &order.expr))
    {
        validate_expression_in_scope(arg, known_fields, &HashSet::new(), false, scope)?;
    }
    Ok(())
}

pub(super) fn validate_expression(
    expr: &Expr,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    validate_expression_in_scope(
        expr,
        known_fields,
        projection_aliases,
        allow_projection_alias,
        ReferenceScope::default(),
    )
}
