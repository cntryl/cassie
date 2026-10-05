//! Destination Boolean context for INSERT VALUES and SELECT projections.

use super::super::{boolean_contexts, coalesce_results::ResultTypes};
use super::{
    BindingContext, CassieError, Catalog, CollectionSchema, DataType, Expr, HashMap, InsertSource,
    SelectItem,
};
use crate::sql::ast::{CommonTableExpression, InsertStatement, QuerySource, SelectStatement};
use crate::types::Schema;

pub(super) fn validate(
    statement: &mut InsertStatement,
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    validate_with_parameters(statement, schema, catalog, context, &[])
}

pub(super) fn validate_with_parameters(
    statement: &mut InsertStatement,
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
) -> Result<(), CassieError> {
    match &mut statement.source {
        InsertSource::Values(rows) => validate_values(
            rows,
            &statement.columns,
            schema,
            catalog,
            context,
            parameter_types,
        ),
        InsertSource::Select(select) => validate_select(
            select,
            &statement.columns,
            schema,
            catalog,
            context,
            &SelectScope {
                schemas: &HashMap::new(),
                ctes: &[],
                parameter_types,
            },
        ),
    }
}

struct SelectScope<'a> {
    schemas: &'a HashMap<String, Schema>,
    ctes: &'a [CommonTableExpression],
    parameter_types: &'a [i32],
}

fn expected_type<'a>(
    index: usize,
    columns: &[String],
    schema: &'a CollectionSchema,
) -> Option<&'a DataType> {
    if columns.is_empty() {
        schema.fields.get(index)
    } else {
        columns
            .get(index)
            .and_then(|name| schema.fields.iter().find(|field| field.name == *name))
    }
    .map(|field| &field.data_type)
}

fn expected_width(columns: &[String], schema: &CollectionSchema) -> usize {
    if columns.is_empty() {
        schema.fields.len()
    } else {
        columns.len()
    }
}

fn validate_values(
    rows: &mut [Vec<Expr>],
    columns: &[String],
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
) -> Result<(), CassieError> {
    if rows
        .iter()
        .any(|row| row.len() != expected_width(columns, schema))
    {
        // The existing executor owns INSERT shape errors.
        return Ok(());
    }
    let types = ResultTypes::for_source_with_parameters(
        &QuerySource::SingleRow,
        &[],
        catalog,
        context,
        parameter_types,
    )?;
    for row in rows {
        for (index, expression) in row.iter_mut().enumerate() {
            boolean_contexts::validate_value(
                expression,
                &types,
                expected_type(index, columns, schema),
                catalog,
                context,
            )?;
        }
    }
    Ok(())
}

fn validate_select(
    select: &mut SelectStatement,
    columns: &[String],
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
    scope: &SelectScope<'_>,
) -> Result<(), CassieError> {
    // Preserve the existing qualified-wildcard restriction and runtime error.
    if select
        .projection
        .iter()
        .any(|item| matches!(item, SelectItem::Column { name, .. } if name.ends_with('*')))
    {
        return Ok(());
    }
    let types = ResultTypes::for_scope_with_parameters(
        &select.source,
        &select.ctes,
        catalog,
        context,
        scope.schemas,
        scope.parameter_types,
    )?;
    let ctes: Vec<_> = scope.ctes.iter().chain(&select.ctes).cloned().collect();
    let wildcard = if select
        .projection
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        let functions = crate::catalog::function_resolution::functions_for_scope(
            &catalog.list_functions(),
            &context.database,
            &context.search_path,
            context.scopes_database_objects(),
        );
        super::super::wildcard::wildcard_output_fields_with_parameters(
            &select.source,
            &ctes,
            catalog,
            &functions,
            scope.parameter_types,
        )?
    } else {
        Vec::new()
    };
    let width: usize = select
        .projection
        .iter()
        .map(|item| {
            if matches!(item, SelectItem::Wildcard) {
                wildcard.len()
            } else {
                1
            }
        })
        .sum();
    if width != expected_width(columns, schema) {
        // Do not change shape-error phase or coerce an invalid INSERT shape.
        return Ok(());
    }
    let mut index = 0;
    for item in &mut select.projection {
        if matches!(item, SelectItem::Wildcard) {
            for field in &wildcard {
                validate_known_type(
                    &field.data_type,
                    &types,
                    expected_type(index, columns, schema),
                    catalog,
                    context,
                )?;
                index += 1;
            }
        } else {
            validate_item(
                item,
                &types,
                expected_type(index, columns, schema),
                catalog,
                context,
            )?;
            index += 1;
        }
    }
    if let Some(set) = &mut select.set {
        validate_select(
            &mut set.right,
            columns,
            schema,
            catalog,
            context,
            &SelectScope {
                schemas: types.cte_schemas(),
                ctes: &ctes,
                parameter_types: scope.parameter_types,
            },
        )?;
    }
    Ok(())
}

fn validate_item(
    item: &mut SelectItem,
    types: &ResultTypes,
    expected: Option<&DataType>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    // Window output types come from the same inferred projected schema as
    // Describe; their arguments were already validated while binding SELECT.
    let window_type = matches!(item, SelectItem::WindowFunction { .. })
        .then(|| types.projection_type(item))
        .flatten();
    match item {
        SelectItem::Expr { expr, .. } => {
            boolean_contexts::validate_value(expr, types, expected, catalog, context)
        }
        SelectItem::Column { name, .. } => boolean_contexts::validate_value(
            &mut Expr::Column(name.clone()),
            types,
            expected,
            catalog,
            context,
        ),
        SelectItem::Function { function, .. } => boolean_contexts::validate_value(
            &mut Expr::Function(function.clone()),
            types,
            expected,
            catalog,
            context,
        ),
        SelectItem::WindowFunction { .. } => match window_type {
            Some(data_type) => validate_known_type(&data_type, types, expected, catalog, context),
            None => Ok(()),
        },
        SelectItem::Wildcard => Ok(()),
    }
}

fn validate_known_type(
    data_type: &DataType,
    types: &ResultTypes,
    expected: Option<&DataType>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    // This temporary expression carries resolved output metadata only. It is
    // never retained in the SQL AST or executed, and is not unknown input.
    let mut expression = Expr::Cast {
        expr: Box::new(Expr::Null),
        data_type: data_type.clone(),
    };
    boolean_contexts::validate_value(&mut expression, types, expected, catalog, context)
}
