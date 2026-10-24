//! Column shape of a `SELECT *` expansion, in the order the executor emits
//! the values.
//!
//! The executor expands `*` from the entries each source row carries (see
//! `executor::projection`), so the described columns must be derived from the
//! same relation shapes: a derived table, CTE, view, table function or join
//! contributes exactly the columns its rows carry. A table without a declared
//! `id` field carries its internal row identity under
//! [`ROW_IDENTITY_COLUMN`]; the fields returned here keep that name so the
//! identity-hiding rule the executor applies to a joined row can be mirrored.

use super::inference::{
    append_source_qualifiers, infer_projection_schema_with_parameters,
    infer_source_schema_with_outer, relation_output_schema,
};
use super::select::table_function_columns;
use super::{
    CassieError, Catalog, CommonTableExpression, CteQuery, DataType, FieldSchema, HashMap, HashSet,
    QuerySource, Schema, SelectItem, SelectStatement,
};
use crate::types::row_identity::{
    is_legacy_id_column, is_row_identity_column, LEGACY_ID_COLUMN, ROW_IDENTITY_COLUMN,
};

type WildcardScope = HashMap<String, Vec<FieldSchema>>;

pub(crate) fn source_row_fields(
    source: &QuerySource,
    scope: &HashMap<String, Vec<FieldSchema>>,
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Vec<FieldSchema>, CassieError> {
    source_fields(source, scope, catalog, user_functions, &[], None)
}

pub(crate) fn cte_row_fields(
    cte: &CommonTableExpression,
    scope: &HashMap<String, Vec<FieldSchema>>,
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Vec<FieldSchema>, CassieError> {
    let resolved = cte_scope(
        std::slice::from_ref(cte),
        scope,
        catalog,
        user_functions,
        &[],
    )?;
    Ok(resolved
        .get(&cte.name.to_ascii_lowercase())
        .cloned()
        .unwrap_or_default())
}

/// Returns the fields `SELECT *` over `source` yields, in row order. The
/// internal row identity of a table without a declared `id` field is reported
/// under its `id` output name.
///
/// # Errors
///
/// Returns an error when a referenced relation or CTE body cannot be inferred.
pub(crate) fn wildcard_output_fields(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Vec<FieldSchema>, CassieError> {
    wildcard_output_fields_with_parameters(source, ctes, catalog, user_functions, &[])
}

/// Returns displayed wildcard fields together with whether the projected row
/// still carries an internal identity after declared-id hiding.
pub(crate) fn wildcard_output_fields_and_identity(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<(Vec<FieldSchema>, bool), CassieError> {
    wildcard_output_shape_with_parameters(source, ctes, catalog, user_functions, &[])
}

pub(super) fn wildcard_output_fields_with_parameters(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Result<Vec<FieldSchema>, CassieError> {
    wildcard_output_shape_with_parameters(source, ctes, catalog, user_functions, parameter_types)
        .map(|(fields, _)| fields)
}

pub(crate) fn wildcard_output_shape_with_parameters(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Result<(Vec<FieldSchema>, bool), CassieError> {
    let scope = cte_scope(
        ctes,
        &WildcardScope::new(),
        catalog,
        user_functions,
        parameter_types,
    )?;
    let fields = hide_shadowed_identity(source_fields(
        source,
        &scope,
        catalog,
        user_functions,
        parameter_types,
        None,
    )?);
    let carries_identity = fields
        .iter()
        .any(|field| is_row_identity_column(&field.name));
    let displayed = fields
        .into_iter()
        .map(|mut field| {
            if is_row_identity_column(&field.name) {
                field.name = LEGACY_ID_COLUMN.to_string();
            }
            field
        })
        .collect();
    Ok((displayed, carries_identity))
}

fn cte_scope(
    ctes: &[CommonTableExpression],
    outer: &WildcardScope,
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Result<WildcardScope, CassieError> {
    let mut scope = outer.clone();
    for cte in ctes {
        let statement = match &cte.query {
            CteQuery::Simple(statement) => statement,
            CteQuery::Recursive { base, .. } => base,
        };
        let Some(select) = super::cte_bodies::select_view(statement) else {
            return Err(CassieError::Planner(
                "CTE body must be a SELECT statement".into(),
            ));
        };
        let mut fields = select_fields(
            &select,
            &scope,
            catalog,
            user_functions,
            parameter_types,
            None,
        )?;
        // Same rule as the executor's CTE row renaming: the column list
        // renames the body's output positionally unless it holds `*`.
        if !cte.aliases.is_empty() && !cte.aliases.iter().any(|alias| alias == "*") {
            for (index, field) in fields.iter_mut().enumerate() {
                if let Some(alias) = cte.aliases.get(index) {
                    field.name.clone_from(alias);
                }
            }
        }
        append_generated_cte_fields(cte, &mut fields, user_functions)?;
        scope.insert(cte.name.to_ascii_lowercase(), fields);
    }
    Ok(scope)
}

fn append_generated_cte_fields(
    cte: &CommonTableExpression,
    fields: &mut Vec<FieldSchema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<(), CassieError> {
    append_search_cycle_fields(
        &cte.name,
        fields,
        cte.search.as_ref(),
        cte.cycle.as_ref(),
        user_functions,
    )
}

pub(super) fn append_search_cycle_fields(
    cte_name: &str,
    fields: &mut Vec<FieldSchema>,
    search: Option<&crate::sql::ast::CteSearch>,
    cycle: Option<&crate::sql::ast::CteCycle>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<(), CassieError> {
    let mut names = fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();
    for key in search
        .into_iter()
        .flat_map(|search| search.by.iter())
        .chain(cycle.into_iter().flat_map(|cycle| cycle.by.iter()))
    {
        if !fields.iter().any(|field| field.name == *key) {
            return Err(CassieError::Planner(format!(
                "CTE '{cte_name}' SEARCH or CYCLE key '{key}' is not an output column"
            )));
        }
    }
    if let Some(cycle) = cycle {
        for key in &cycle.by {
            let field = fields
                .iter()
                .find(|field| field.name == *key)
                .ok_or_else(|| {
                    CassieError::Planner(format!(
                        "CTE '{cte_name}' CYCLE key '{key}' is not an output column"
                    ))
                })?;
            if !crate::sql::functions::has_distinct_equality(&field.data_type) {
                return Err(CassieError::Unsupported(format!(
                    "CYCLE key type '{}' does not support equality",
                    field.data_type.type_name()
                )));
            }
        }
    }
    if let Some(search) = search {
        push_generated_field(
            &mut names,
            fields,
            cte_name,
            &search.sequence_column,
            DataType::Text,
        )?;
    }
    if let Some(cycle) = cycle {
        let source = Schema {
            fields: fields.clone(),
        };
        let mark_type =
            super::inference::infer_expr_type(&cycle.mark_value, &source, user_functions, &[])
                .ok_or_else(|| CassieError::Planner("cannot infer CYCLE mark type".into()))?;
        let default_type =
            super::inference::infer_expr_type(&cycle.default_value, &source, user_functions, &[])
                .ok_or_else(|| CassieError::Planner("cannot infer CYCLE default type".into()))?;
        let mark_type = match (mark_type, default_type) {
            (DataType::Null, DataType::Null) => DataType::Text,
            (DataType::Null, other) | (other, DataType::Null) => other,
            (mark, default) if mark == default => mark,
            (mark, default) => {
                super::inference::common_case_type(mark, default).ok_or_else(|| {
                    CassieError::Planner(
                        "CYCLE mark and default values have incompatible types".into(),
                    )
                })?
            }
        };
        if !cycle_mark_type_has_inequality_operator(&mark_type) {
            return Err(CassieError::Unsupported(format!(
                "CYCLE mark type '{}' does not support inequality",
                mark_type.type_name()
            )));
        }
        push_generated_field(&mut names, fields, cte_name, &cycle.mark_column, mark_type)?;
        push_generated_field(
            &mut names,
            fields,
            cte_name,
            &cycle.path_column,
            DataType::Text,
        )?;
    }
    Ok(())
}

fn cycle_mark_type_has_inequality_operator(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Boolean
            | DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Text
            | DataType::Char { .. }
            | DataType::Varchar { .. }
            | DataType::Uuid
            | DataType::Extended(
                crate::types::extended::ExtendedType::Numeric(_)
                    | crate::types::extended::ExtendedType::Real
            )
    )
}

fn push_generated_field(
    names: &mut HashSet<String>,
    fields: &mut Vec<FieldSchema>,
    cte_name: &str,
    name: &str,
    data_type: DataType,
) -> Result<(), CassieError> {
    if !names.insert(name.to_owned()) {
        return Err(CassieError::Planner(format!(
            "CTE '{cte_name}' generated column '{name}' duplicates an output column"
        )));
    }
    fields.push(FieldSchema {
        name: name.to_owned(),
        data_type,
        nullable: false,
    });
    Ok(())
}

fn select_fields(
    select: &SelectStatement,
    outer: &WildcardScope,
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
    outer_fields: Option<&Schema>,
) -> Result<Vec<FieldSchema>, CassieError> {
    let scope = cte_scope(
        &select.ctes,
        outer,
        catalog,
        user_functions,
        parameter_types,
    )?;
    let mut fields = Vec::new();
    let mut lookup: Option<Schema> = None;
    for item in &select.projection {
        if matches!(item, SelectItem::Wildcard) {
            let source = source_fields(
                &select.source,
                &scope,
                catalog,
                user_functions,
                parameter_types,
                outer_fields,
            )?;
            fields.extend(hide_shadowed_identity(source));
            continue;
        }
        if lookup.is_none() {
            let cte_schemas = lookup_cte_schemas(&scope);
            let mut schema = infer_source_schema_with_outer(
                &select.source,
                catalog,
                &cte_schemas,
                user_functions,
                false,
                parameter_types,
                outer_fields,
            )?;
            if let Some(outer) = outer_fields {
                // Inner names precede outer names; outer fields are available
                // only for typing explicit items, never wildcard expansion.
                append_source_qualifiers(&select.source, catalog, &mut schema)?;
                schema.fields.extend(outer.fields.iter().cloned());
            }
            lookup = Some(schema);
        }
        if let Some(schema) = &lookup {
            fields.extend(
                infer_projection_schema_with_parameters(
                    std::slice::from_ref(item),
                    schema,
                    user_functions,
                    parameter_types,
                )
                .fields,
            );
        }
    }
    Ok(fields)
}

fn source_fields(
    source: &QuerySource,
    scope: &WildcardScope,
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
    outer_fields: Option<&Schema>,
) -> Result<Vec<FieldSchema>, CassieError> {
    match source {
        QuerySource::Values { rows } => {
            Ok(super::values::schema(rows, user_functions, parameter_types)?.fields)
        }

        QuerySource::Aliased { source, .. } => source_fields(
            source,
            scope,
            catalog,
            user_functions,
            parameter_types,
            outer_fields,
        ),
        QuerySource::Collection(name) => {
            if let Some(fields) = scope.get(&name.to_ascii_lowercase()) {
                return Ok(fields.clone());
            }
            relation_fields(catalog, name)
        }
        QuerySource::Cte(name) => {
            if let Some(fields) = scope.get(&name.to_ascii_lowercase()) {
                return Ok(fields.clone());
            }
            relation_fields(catalog, name)
        }
        QuerySource::SingleRow => Ok(Vec::new()),
        QuerySource::TableFunction { name, .. } => Ok(table_function_columns(name)
            .into_iter()
            .map(|(name, data_type)| FieldSchema {
                name,
                data_type,
                nullable: true,
            })
            .collect()),
        QuerySource::Subquery {
            select, lateral, ..
        } => select_fields(
            select,
            scope,
            catalog,
            user_functions,
            parameter_types,
            if *lateral { outer_fields } else { None },
        ),
        QuerySource::Join { left, right, .. } => {
            let mut fields = source_fields(
                left,
                scope,
                catalog,
                user_functions,
                parameter_types,
                outer_fields,
            )?;
            let lateral_scope = if source_consumes_outer_fields(right) {
                Some(lateral_lookup(
                    left,
                    scope,
                    catalog,
                    user_functions,
                    parameter_types,
                    outer_fields,
                )?)
            } else {
                None
            };
            fields.extend(source_fields(
                right,
                scope,
                catalog,
                user_functions,
                parameter_types,
                lateral_scope.as_ref(),
            )?);
            Ok(fields)
        }
    }
}

fn source_consumes_outer_fields(source: &QuerySource) -> bool {
    match source {
        QuerySource::Aliased { source, .. } => source_consumes_outer_fields(source),
        QuerySource::Subquery { lateral, .. } => *lateral,
        QuerySource::Join { left, right, .. } => {
            source_consumes_outer_fields(left) || source_consumes_outer_fields(right)
        }
        QuerySource::Collection(_)
        | QuerySource::Cte(_)
        | QuerySource::Values { .. }
        | QuerySource::SingleRow
        | QuerySource::TableFunction { .. } => false,
    }
}

fn lateral_lookup(
    left: &QuerySource,
    scope: &WildcardScope,
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
    outer_fields: Option<&Schema>,
) -> Result<Schema, CassieError> {
    let cte_schemas = lookup_cte_schemas(scope);
    let mut lookup = infer_source_schema_with_outer(
        left,
        catalog,
        &cte_schemas,
        user_functions,
        true,
        parameter_types,
        outer_fields,
    )?;
    append_source_qualifiers(left, catalog, &mut lookup)?;
    if let Some(outer) = outer_fields {
        lookup.fields.extend(outer.fields.iter().cloned());
    }
    Ok(lookup)
}

fn lookup_cte_schemas(scope: &WildcardScope) -> HashMap<String, Schema> {
    scope
        .iter()
        .map(|(name, fields)| {
            let fields = fields
                .iter()
                .cloned()
                .map(|mut field| {
                    if is_row_identity_column(&field.name) {
                        field.name = LEGACY_ID_COLUMN.to_string();
                    }
                    field
                })
                .collect();
            (name.clone(), Schema { fields })
        })
        .collect()
}

fn relation_fields(catalog: &Catalog, name: &str) -> Result<Vec<FieldSchema>, CassieError> {
    let mut fields = relation_output_schema(catalog, name)?.fields;
    // Only a base table without a declared `id` field carries the internal
    // row identity; views, materialized projections and catalog views emit
    // their declared columns as-is.
    let carries_row_identity = crate::catalog::virtual_views::schema(name).is_none()
        && catalog.get_view(name).is_none()
        && catalog.get_materialized_projection(name).is_none()
        && catalog
            .get_schema(name)
            .is_some_and(|schema| !schema.declares_id());
    if carries_row_identity {
        if let Some(first) = fields.first_mut() {
            first.name = ROW_IDENTITY_COLUMN.to_string();
        }
    }
    Ok(fields)
}

/// Mirrors the executor's wildcard rule: a row that already carries a real
/// `id` column keeps every internal row identity out of `SELECT *`.
fn hide_shadowed_identity(fields: Vec<FieldSchema>) -> Vec<FieldSchema> {
    if fields.iter().any(|field| is_legacy_id_column(&field.name)) {
        fields
            .into_iter()
            .filter(|field| !is_row_identity_column(&field.name))
            .collect()
    } else {
        fields
    }
}

#[cfg(test)]
mod cycle_key_tests {
    use super::append_search_cycle_fields;
    use crate::sql::ast::{CteCycle, Expr};
    use crate::types::{DataType, FieldSchema};

    #[test]
    fn should_reject_cycle_key_arrays_without_element_equality() {
        // Arrange
        let cycle = CteCycle {
            by: vec!["node".to_owned()],
            mark_column: "is_cycle".to_owned(),
            mark_value: Expr::BoolLiteral(true),
            default_value: Expr::BoolLiteral(false),
            path_column: "path".to_owned(),
        };
        let mut fields = vec![FieldSchema {
            name: "node".to_owned(),
            data_type: DataType::Array(Box::new(DataType::Json)),
            nullable: false,
        }];

        // Act
        let result = append_search_cycle_fields(
            "walk",
            &mut fields,
            None,
            Some(&cycle),
            &std::collections::HashMap::new(),
        );

        // Assert
        assert!(result
            .expect_err("array element type JSON has no PostgreSQL equality operator")
            .to_string()
            .contains("CYCLE key type 'json[]' does not support equality"));
    }
}
