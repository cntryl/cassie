#[path = "inference_types.rs"]
mod types;
pub(crate) use types::common_case_type;

use super::{
    virtual_views, BinaryOp, CassieError, Catalog, CommonTableExpression, CteQuery, DataType, Expr,
    FieldSchema, FunctionCall, HashMap, QuerySource, Schema, SelectItem, SelectStatement,
};

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn infer_select_schema(
    select: &SelectStatement,
    catalog: &Catalog,
) -> Result<Schema, CassieError> {
    infer_select_schema_with_ctes(select, catalog, &HashMap::new())
}

pub(super) fn infer_select_schema_with_ctes(
    select: &SelectStatement,
    catalog: &Catalog,
    ctes: &HashMap<String, Schema>,
) -> Result<Schema, CassieError> {
    let user_functions = crate::catalog::function_resolution::functions_for_scope(
        &catalog.list_functions(),
        "postgres",
        &[crate::catalog::DEFAULT_SCHEMA.to_string()],
        false,
    );

    infer_select_schema_with_scope(select, catalog, ctes, &user_functions)
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn infer_select_schema_with_context(
    select: &SelectStatement,
    catalog: &Catalog,
    context: &super::BindingContext,
) -> Result<Schema, CassieError> {
    let user_functions = crate::catalog::function_resolution::functions_for_scope(
        &catalog.list_functions(),
        &context.database,
        &context.search_path,
        context.scopes_database_objects(),
    );

    infer_select_schema_with_scope(select, catalog, &HashMap::new(), &user_functions)
}

pub(super) fn infer_select_schema_with_scope(
    select: &SelectStatement,
    catalog: &Catalog,
    outer_ctes: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Schema, CassieError> {
    infer_select_schema_with_parameters(select, catalog, outer_ctes, user_functions, &[])
}

pub(super) fn infer_select_schema_with_parameters(
    select: &SelectStatement,
    catalog: &Catalog,
    outer_ctes: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Result<Schema, CassieError> {
    infer_select_schema_with_outer(
        select,
        catalog,
        outer_ctes,
        user_functions,
        parameter_types,
        None,
    )
}

pub(super) fn infer_select_schema_with_outer(
    select: &SelectStatement,
    catalog: &Catalog,
    outer_ctes: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
    outer_fields: Option<&Schema>,
) -> Result<Schema, CassieError> {
    let mut cte_schemas = outer_ctes.clone();
    for cte in &select.ctes {
        let schema = infer_cte_schema_with_parameters(
            cte,
            catalog,
            &cte_schemas,
            user_functions,
            parameter_types,
        )?;
        cte_schemas.insert(cte.name.to_ascii_lowercase(), schema);
    }

    let source_schema = infer_source_schema_with_outer(
        &select.source,
        catalog,
        &cte_schemas,
        user_functions,
        false,
        parameter_types,
        outer_fields,
    )?;
    let lookup = if let Some(outer) = outer_fields {
        let mut lookup = source_schema.clone();
        append_source_qualifiers(&select.source, catalog, &mut lookup)?;
        // The inner source owns unqualified names; explicit outer names remain
        // available only in this inference lookup, never in wildcard output.
        lookup.fields.extend(outer.fields.iter().cloned());
        lookup
    } else {
        source_schema.clone()
    };
    let cte_scope = cte_schemas
        .iter()
        .map(|(name, schema)| {
            let row_fields = schema.fields.clone();
            let visible = row_fields.iter().map(|field| field.name.clone()).collect();
            (
                name.clone(),
                super::CteBinding {
                    visible,
                    row_fields,
                },
            )
        })
        .collect();
    let wildcard_context = QualifiedWildcardContext {
        source: &select.source,
        catalog,
        cte_scope: &cte_scope,
        lookup: &lookup,
    };
    let mut fields = infer_select_projection_schema(
        &select.projection,
        &source_schema,
        user_functions,
        parameter_types,
        &wildcard_context,
    )?;

    if let Some(set) = &select.set {
        let right_schema = infer_select_schema_with_outer(
            &set.right,
            catalog,
            &cte_schemas,
            user_functions,
            parameter_types,
            outer_fields,
        )?;
        if fields.fields.len() != right_schema.fields.len() {
            return Err(CassieError::Planner(format!(
                "set operation column count mismatch: {} != {}",
                fields.fields.len(),
                right_schema.fields.len()
            )));
        }
    }

    for group_expr in &select.group_by {
        if let Expr::Column(name) = group_expr {
            let _ = schema_field_type(&source_schema, name);
        }
    }

    fields.fields.iter_mut().for_each(|field| {
        field.nullable = true;
    });

    Ok(fields)
}

/// Resolves the output schema of the CTE named `name`, as a collection schema
/// for result metadata.
///
/// A CTE is not a catalog object, so `Catalog::get_schema` finds nothing for it
/// and result metadata used to fall back to `text` for every column. Earlier
/// CTEs are resolved first because a later one may select from them.
///
/// # Errors
///
/// Returns `None` when no CTE of that name is in scope, or when its body
/// cannot be inferred; callers then keep their existing fallback.
#[must_use]
pub fn cte_collection_schema(
    ctes: &[CommonTableExpression],
    name: &str,
    catalog: &Catalog,
) -> Option<crate::catalog::CollectionSchema> {
    let functions = catalog.list_functions();
    let user_functions = crate::catalog::function_resolution::functions_for_scope(
        &functions,
        "postgres",
        &[crate::catalog::DEFAULT_SCHEMA.to_string()],
        false,
    );

    cte_collection_schema_with_functions(ctes, name, catalog, &user_functions)
}

/// Infers a CTE-backed collection schema with the caller's visible functions.
#[must_use]
pub(crate) fn cte_collection_schema_with_functions(
    ctes: &[CommonTableExpression],
    name: &str,
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Option<crate::catalog::CollectionSchema> {
    let wanted = name.to_ascii_lowercase();

    let mut in_scope: HashMap<String, Schema> = HashMap::new();
    for cte in ctes {
        let schema = infer_cte_schema(cte, catalog, &in_scope, user_functions).ok()?;
        let cte_name = cte.name.to_ascii_lowercase();
        if cte_name == wanted {
            return Some(crate::catalog::CollectionSchema {
                collection: name.to_string(),
                fields: schema
                    .fields
                    .into_iter()
                    .map(|field| crate::catalog::FieldMeta {
                        name: field.name,
                        data_type: field.data_type,
                        is_indexed: false,
                        boost: None,
                    })
                    .collect(),
            });
        }
        in_scope.insert(cte_name, schema);
    }
    None
}

pub(super) fn infer_cte_schema(
    cte: &CommonTableExpression,
    catalog: &Catalog,
    cte_schemas: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Schema, CassieError> {
    infer_cte_schema_with_parameters(cte, catalog, cte_schemas, user_functions, &[])
}

pub(super) fn infer_cte_schema_with_parameters(
    cte: &CommonTableExpression,
    catalog: &Catalog,
    cte_schemas: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Result<Schema, CassieError> {
    let query = match &cte.query {
        CteQuery::Simple(statement) => statement,
        CteQuery::Recursive { base, .. } => base,
    };

    let Some(select) = super::cte_bodies::select_view(query) else {
        return Err(CassieError::Planner(
            "CTE body must be a SELECT statement".into(),
        ));
    };

    let mut schema = infer_select_schema_with_parameters(
        &select,
        catalog,
        cte_schemas,
        user_functions,
        parameter_types,
    )?;

    let recursive = matches!(cte.query, CteQuery::Recursive { .. });
    let placeholder = cte.aliases.len() == 1 && cte.aliases[0] == "*";
    if !cte.aliases.is_empty() && !placeholder {
        if cte.aliases.len() > schema.fields.len()
            || (recursive && schema.fields.len() != cte.aliases.len())
        {
            return Err(CassieError::Planner(format!(
                "CTE '{}' alias count does not match output columns",
                cte.name
            )));
        }

        for (field, alias) in schema.fields.iter_mut().zip(cte.aliases.iter()) {
            field.name.clone_from(alias);
        }
    }

    Ok(schema)
}

pub(super) fn infer_source_schema_with_parameters(
    source: &QuerySource,
    catalog: &Catalog,
    cte_schemas: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    qualify: bool,
    parameter_types: &[i32],
) -> Result<Schema, CassieError> {
    infer_source_schema_with_outer(
        source,
        catalog,
        cte_schemas,
        user_functions,
        qualify,
        parameter_types,
        None,
    )
}

pub(super) fn infer_source_schema_with_outer(
    source: &QuerySource,
    catalog: &Catalog,
    cte_schemas: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    qualify: bool,
    parameter_types: &[i32],
    outer_fields: Option<&Schema>,
) -> Result<Schema, CassieError> {
    let schema = match source {
        QuerySource::Values { rows } => {
            super::values::schema(rows, user_functions, parameter_types)?
        }

        QuerySource::Aliased { source, alias, .. } => {
            let mut schema = infer_source_schema_with_outer(
                source,
                catalog,
                cte_schemas,
                user_functions,
                false,
                parameter_types,
                outer_fields,
            )?;
            preserve_alias_identity(source, catalog, &mut schema);
            qualify_schema(&schema, &super::aliases::qualifier(alias))
        }
        QuerySource::Collection(name) => relation_output_schema(catalog, name)?,
        QuerySource::Cte(name) => cte_schemas
            .get(&name.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?,
        QuerySource::SingleRow => Schema { fields: Vec::new() },
        QuerySource::TableFunction { name, .. } => Schema {
            fields: super::select::table_function_columns(name)
                .into_iter()
                .map(|(name, data_type)| FieldSchema {
                    name,
                    data_type,
                    nullable: true,
                })
                .collect(),
        },
        QuerySource::Subquery {
            alias,
            select,
            lateral,
        } => {
            let outer = if *lateral { outer_fields } else { None };
            let inner = infer_select_schema_with_outer(
                select,
                catalog,
                cte_schemas,
                user_functions,
                parameter_types,
                outer,
            )?;
            qualify_schema(&inner, alias)
        }
        QuerySource::Join { left, right, .. } => {
            let left_schema = infer_source_schema_with_outer(
                left,
                catalog,
                cte_schemas,
                user_functions,
                true,
                parameter_types,
                outer_fields,
            )?;
            let mut lateral_scope = left_schema.clone();
            append_source_qualifiers(left, catalog, &mut lateral_scope)?;
            if let Some(outer) = outer_fields {
                lateral_scope.fields.extend(outer.fields.iter().cloned());
            }
            let right_schema = infer_source_schema_with_outer(
                right,
                catalog,
                cte_schemas,
                user_functions,
                true,
                parameter_types,
                Some(&lateral_scope),
            )?;
            let mut fields = left_schema.fields;
            fields.extend(right_schema.fields);
            Schema { fields }
        }
    };

    if qualify {
        Ok(match source {
            QuerySource::Collection(name) => qualify_schema(&schema, name),
            QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
                qualify_schema(&schema, name)
            }
            QuerySource::SingleRow
            | QuerySource::Values { .. }
            | QuerySource::Aliased { .. }
            | QuerySource::Subquery { .. }
            | QuerySource::Join { .. } => schema,
        })
    } else {
        Ok(schema)
    }
}

pub(super) fn append_source_qualifiers(
    source: &crate::sql::ast::QuerySource,
    catalog: &Catalog,
    schema: &mut Schema,
) -> Result<(), CassieError> {
    match source {
        crate::sql::ast::QuerySource::Collection(name) => {
            let fields = relation_output_schema(catalog, name)?;
            for field in fields.fields {
                let key =
                    crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
                for qualifier in crate::catalog::qualifier_variants(name) {
                    schema.fields.push(crate::types::FieldSchema {
                        name: format!("{qualifier}.{key}"),
                        data_type: field.data_type.clone(),
                        nullable: field.nullable,
                    });
                }
            }
        }
        crate::sql::ast::QuerySource::Join { left, right, .. } => {
            append_source_qualifiers(left, catalog, schema)?;
            append_source_qualifiers(right, catalog, schema)?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn relation_output_schema(catalog: &Catalog, name: &str) -> Result<Schema, CassieError> {
    if let Some(fields) = virtual_views::schema(name) {
        return Ok(Schema {
            fields: fields
                .into_iter()
                .map(|(field_name, data_type)| FieldSchema {
                    name: field_name,
                    data_type,
                    nullable: true,
                })
                .collect(),
        });
    }

    if let Some(view) = catalog.get_view(name) {
        crate::sql::definition_guard::query_sql(&view.query)?;
        return Ok(view.schema);
    }

    if let Some(projection) = catalog.get_materialized_projection(name) {
        let materialized = projection.materialized.ok_or_else(|| {
            CassieError::Planner(format!(
                "materialized projection '{name}' is missing output schema"
            ))
        })?;
        return Ok(materialized.output_schema);
    }

    let schema = catalog
        .get_schema(name)
        .ok_or_else(|| CassieError::CollectionNotFound(name.to_string()))?;

    let mut fields = Vec::with_capacity(schema.fields.len() + 1);
    if !schema.declares_id() {
        fields.push(FieldSchema {
            name: crate::types::row_identity::LEGACY_ID_COLUMN.to_string(),
            data_type: DataType::Text,
            nullable: true,
        });
    }
    fields.extend(schema.fields.into_iter().map(|field| FieldSchema {
        name: field.name,
        data_type: field.data_type,
        nullable: true,
    }));

    Ok(Schema { fields })
}

pub(super) fn qualify_schema(schema: &Schema, qualifier: &str) -> Schema {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut fields = Vec::with_capacity(schema.fields.len() * 2);
    for field in &schema.fields {
        fields.push(field.clone());
        fields.push(FieldSchema {
            name: format!("{qualifier}.{}", field.name),
            data_type: field.data_type.clone(),
            nullable: field.nullable,
        });
    }
    Schema { fields }
}

pub(super) fn infer_projection_schema_with_parameters(
    projection: &[SelectItem],
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Schema {
    let mut fields = Vec::new();
    for item in projection {
        match item {
            SelectItem::Wildcard => fields.extend(source_schema.fields.iter().cloned()),
            SelectItem::Column { name, alias } => {
                let output_name = alias.clone().unwrap_or_else(|| {
                    crate::sql::ColumnIdentifierPath::parse(name)
                        .map_or_else(|_| name.clone(), |column| column.implicit_output_label())
                });
                fields.push(FieldSchema {
                    name: output_name,
                    data_type: schema_field_type(source_schema, name).unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::Function { function, alias } => {
                let output_name = alias
                    .as_deref()
                    .unwrap_or(function.name.as_str())
                    .to_string();
                fields.push(FieldSchema {
                    name: output_name,
                    data_type: infer_function_return_type(
                        function,
                        source_schema,
                        user_functions,
                        parameter_types,
                    )
                    .unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::Expr { expr, alias } => {
                fields.push(FieldSchema {
                    name: alias.as_deref().unwrap_or("expr").to_string(),
                    data_type: infer_expr_type(
                        expr,
                        source_schema,
                        user_functions,
                        parameter_types,
                    )
                    .unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::WindowFunction { function, alias } => {
                let data_type = super::infer_window_return_type(
                    function,
                    source_schema,
                    user_functions,
                    parameter_types,
                );
                fields.push(FieldSchema {
                    name: alias
                        .as_deref()
                        .unwrap_or(function.name.as_str())
                        .to_string(),
                    data_type,
                    nullable: false,
                });
            }
        }
    }

    Schema { fields }
}

pub(crate) fn infer_projection_expr_type(
    expr: &Expr,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    match expr {
        Expr::Binary {
            op: BinaryOp::Integer { operation, .. },
            ..
        } if !operation.is_shift() => Some(DataType::BigInt),
        Expr::Binary {
            op: BinaryOp::Integer { width, .. },
            ..
        } => Some(width.data_type()),
        _ => infer_expr_type(expr, source_schema, user_functions, parameter_types),
    }
}

struct QualifiedWildcardContext<'a, 'scope> {
    source: &'a QuerySource,
    catalog: &'a Catalog,
    cte_scope: &'a super::CteScope<'scope>,
    lookup: &'a Schema,
}

fn infer_select_projection_schema(
    projection: &[SelectItem],
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
    wildcard_context: &QualifiedWildcardContext<'_, '_>,
) -> Result<Schema, CassieError> {
    let mut fields = Vec::new();
    for item in projection {
        let expanded = match item {
            SelectItem::Wildcard => {
                fields.extend(source_schema.fields.iter().cloned());
                continue;
            }
            SelectItem::Column { name, .. }
                if crate::sql::ColumnIdentifierPath::is_qualified_wildcard(name) =>
            {
                super::aliases::qualified_wildcard_projection_items(
                    name,
                    wildcard_context.source,
                    wildcard_context.catalog,
                    wildcard_context.cte_scope,
                    wildcard_context.lookup,
                )?
            }
            _ => std::slice::from_ref(item).to_vec(),
        };
        fields.extend(
            infer_projection_schema_with_parameters(
                &expanded,
                wildcard_context.lookup,
                user_functions,
                parameter_types,
            )
            .fields,
        );
    }
    Ok(Schema { fields })
}

pub(super) fn schema_field_type(schema: &Schema, name: &str) -> Option<DataType> {
    let column = crate::sql::ColumnIdentifierPath::parse(name).ok()?;
    let field_key = column.field_lookup_key();
    schema
        .fields
        .iter()
        .find(|field| {
            column.is_qualified()
                && crate::sql::ColumnIdentifierPath::parse(&field.name).is_ok_and(|candidate| {
                    candidate.is_qualified() && candidate.lookup_key() == column.lookup_key()
                })
        })
        .or_else(|| {
            schema.fields.iter().find(|field| {
                crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key()
                    == field_key
            })
        })
        .map(|field| field.data_type.clone())
}

#[path = "inference_numeric.rs"]
mod numeric;
use numeric::{arithmetic_type, numeric_function_type, power_type};

pub(crate) fn infer_function_return_type(
    function: &FunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let name = function.name.to_ascii_lowercase();
    if let Some(data_type) =
        types::temporal_function_type(function, source_schema, user_functions, parameter_types)
            .or_else(|| {
                super::extended_inference::function_type(
                    function,
                    source_schema,
                    user_functions,
                    parameter_types,
                )
                .or_else(|| {
                    numeric_function_type(function, source_schema, user_functions, parameter_types)
                })
            })
    {
        return Some(data_type);
    }
    if let Some(data_type) = primitive_cast_return_type(
        function,
        &name,
        source_schema,
        user_functions,
        parameter_types,
    ) {
        return Some(data_type);
    }
    if let Some(metadata) = user_functions.get(&name).filter(|metadata| {
        !crate::sql::functions::has_distinct_builtin_arity(
            &name,
            function.args.len(),
            metadata.args.len(),
        )
    }) {
        return Some(metadata.return_type.clone());
    }
    let metadata = crate::sql::functions::function(&name)?;
    if let Some(data_type) = selected_ordered_set_return_type(
        function,
        metadata.id,
        source_schema,
        user_functions,
        parameter_types,
    ) {
        return Some(data_type);
    }
    match metadata.return_type {
        crate::sql::functions::FunctionReturnType::Float => Some(DataType::Float),
        crate::sql::functions::FunctionReturnType::Text => Some(DataType::Text),
        crate::sql::functions::FunctionReturnType::Bytea => Some(DataType::Bytea),
        crate::sql::functions::FunctionReturnType::Int => Some(DataType::Int),
        crate::sql::functions::FunctionReturnType::BigInt => Some(DataType::BigInt),
        crate::sql::functions::FunctionReturnType::Boolean => Some(DataType::Boolean),
        crate::sql::functions::FunctionReturnType::Timestamp => Some(DataType::Timestamp),
        crate::sql::functions::FunctionReturnType::ObjectId(kind) => Some(DataType::ObjectId(kind)),
        crate::sql::functions::FunctionReturnType::FirstComparedArgument => {
            first_compared_argument_return_type(
                function,
                &name,
                source_schema,
                user_functions,
                parameter_types,
            )
        }
        crate::sql::functions::FunctionReturnType::FirstNonNullArgument => {
            let argument_types = function
                .args
                .iter()
                .filter_map(|arg| {
                    infer_expr_type(arg, source_schema, user_functions, parameter_types)
                })
                .filter(|data_type| !matches!(data_type, DataType::Null))
                .collect::<Vec<_>>();
            // Any argument may supply the value (COALESCE), so the result is
            // their common type; incompatible arguments keep the first type.
            argument_types
                .iter()
                .cloned()
                .try_fold(DataType::Null, common_case_type)
                .filter(|data_type| !matches!(data_type, DataType::Null))
                .or_else(|| argument_types.first().cloned())
                .or(Some(DataType::Text))
        }
        crate::sql::functions::FunctionReturnType::NumericArgument => function
            .args
            .first()
            .and_then(|expr| infer_expr_type(expr, source_schema, user_functions, parameter_types))
            .map(|data_type| match data_type {
                DataType::SmallInt => DataType::SmallInt,
                DataType::Int => DataType::Int,
                DataType::BigInt => DataType::BigInt,
                _ => DataType::Float,
            })
            .or(Some(DataType::Float)),
        crate::sql::functions::FunctionReturnType::SumArgument => function
            .args
            .first()
            .and_then(|expr| infer_expr_type(expr, source_schema, user_functions, parameter_types))
            .map(|data_type| crate::types::extended::numeric_types::sum(&data_type))
            .or(Some(DataType::Float)),
        crate::sql::functions::FunctionReturnType::Math
        | crate::sql::functions::FunctionReturnType::Unknown => None,
    }
}

fn first_compared_argument_return_type(
    function: &FunctionCall,
    name: &str,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let argument_types = function
        .args
        .iter()
        .map(|argument| {
            infer_expr_type(argument, source_schema, user_functions, parameter_types)
                .unwrap_or(DataType::Null)
        })
        .collect::<Vec<_>>();
    super::conditional_types::result_type(name, &argument_types).ok()
}

fn primitive_cast_return_type(
    function: &FunctionCall,
    name: &str,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let target = crate::sql::functions::primitive_casts::candidate(function)?;
    let source = infer_expr_type(
        &function.args[0],
        source_schema,
        user_functions,
        parameter_types,
    );
    let exact = user_functions.get(name).is_some_and(|metadata| {
        metadata.args.len() == 1 && source.as_ref() == Some(&metadata.args[0].data_type)
    });
    (crate::sql::functions::primitive_casts::unknown_literal(&function.args[0])
        || !exact && (!user_functions.contains_key(name) || source.is_some()))
    .then_some(target.data_type)
}

fn selected_ordered_set_return_type(
    function: &FunctionCall,
    id: crate::sql::functions::FunctionId,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    if !matches!(
        id,
        crate::sql::functions::FunctionId::Mode
            | crate::sql::functions::FunctionId::PercentileCont
            | crate::sql::functions::FunctionId::PercentileDisc
    ) {
        return None;
    }
    ordered_set_return_type(function, id, source_schema, user_functions, parameter_types)
}

fn ordered_set_return_type(
    function: &FunctionCall,
    id: crate::sql::functions::FunctionId,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let order_type = function.order_by.first().and_then(|order| {
        infer_expr_type(&order.expr, source_schema, user_functions, parameter_types)
    })?;
    if id == crate::sql::functions::FunctionId::Mode {
        return Some(order_type);
    }
    let result_element = match id {
        crate::sql::functions::FunctionId::PercentileCont
            if matches!(
                order_type,
                DataType::Extended(crate::types::extended::ExtendedType::Interval { .. })
            ) =>
        {
            order_type
        }
        crate::sql::functions::FunctionId::PercentileCont => DataType::Float,
        crate::sql::functions::FunctionId::PercentileDisc => order_type,
        _ => return None,
    };
    let fraction_is_array = function.args.first().is_some_and(|fraction| {
        matches!(
            fraction,
            Expr::Subquery(subquery)
                if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array)
        ) || infer_expr_type(fraction, source_schema, user_functions, parameter_types)
            .is_some_and(|data_type| matches!(data_type, DataType::Array(_)))
    });
    Some(if fraction_is_array {
        DataType::Array(Box::new(result_element))
    } else {
        result_element
    })
}

pub(crate) fn infer_expr_type(
    expr: &Expr,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    match expr {
        Expr::Column(name) => schema_field_type(source_schema, name),
        Expr::Case {
            branches,
            else_expr,
            ..
        } => {
            let mut result = DataType::Null;
            for (_, value) in branches {
                result = common_case_type(
                    result,
                    infer_expr_type(value, source_schema, user_functions, parameter_types)?,
                )?;
            }
            if let Some(value) = else_expr {
                result = common_case_type(
                    result,
                    infer_expr_type(value, source_schema, user_functions, parameter_types)?,
                )?;
            }
            Some(result)
        }
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::Function(function) => {
            infer_function_return_type(function, source_schema, user_functions, parameter_types)
        }
        Expr::NumericLiteral(_) => Some(numeric::NUMERIC),
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::UnaryNumeric { expr, op } => super::unary_numeric::result_type(
            *op,
            expr,
            infer_expr_type(expr, source_schema, user_functions, parameter_types),
        ),
        Expr::IntegerNegate { width, .. } => Some(width.data_type()),
        Expr::IntegerLiteral(value) => Some(integer_literal_type(*value)),
        Expr::RowCompare(_)
        | Expr::BoolLiteral(_)
        | Expr::Exists(_)
        | Expr::IsNull { .. }
        | Expr::TruthTest { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::Not { .. } => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        Expr::Subquery(subquery) => subquery.result_type(),
        Expr::Param(index) => parameter_types
            .get(*index)
            .copied()
            .and_then(data_type_for_parameter_oid)
            .or(Some(DataType::Null)),
        Expr::Binary { left, op, right } => infer_binary_type(
            left,
            op,
            right,
            source_schema,
            user_functions,
            parameter_types,
        ),
    }
}

fn infer_binary_type(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    super::extended_inference::binary_type(
        left,
        op,
        right,
        source_schema,
        user_functions,
        parameter_types,
    )
    .or_else(|| match op {
        BinaryOp::Typed { .. }
        | BinaryOp::Quantified { .. }
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::IsDistinctFrom
        | BinaryOp::IsNotDistinctFrom
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte
        | BinaryOp::Like => Some(DataType::Boolean),
        BinaryOp::JsonbKeyExists | BinaryOp::JsonbKeyExistsAny | BinaryOp::JsonbKeyExistsAll => {
            Some(DataType::Boolean)
        }
        BinaryOp::Sub
            if infer_expr_type(left, source_schema, user_functions, parameter_types)
                == Some(DataType::Json) =>
        {
            Some(DataType::Json)
        }
        BinaryOp::Power => Some(power_type(
            left,
            right,
            source_schema,
            user_functions,
            parameter_types,
        )),
        BinaryOp::Concat => types::infer_concatenation_type(
            left,
            right,
            source_schema,
            user_functions,
            parameter_types,
        ),
        BinaryOp::ByteaConcat => Some(DataType::Bytea),
        BinaryOp::Bitwise { operation } if operation.is_shift() => {
            infer_expr_type(left, source_schema, user_functions, parameter_types)
        }
        BinaryOp::Integer { width, .. } => Some(width.data_type()),
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Modulo
        | BinaryOp::Bitwise { .. } => types::temporal_arithmetic_type(
            (left, op, right),
            source_schema,
            user_functions,
            parameter_types,
        )
        .or_else(|| {
            Some(arithmetic_type(
                left,
                right,
                matches!(op, BinaryOp::Modulo),
                source_schema,
                user_functions,
                parameter_types,
            ))
        }),
        BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
            Some(DataType::Float)
        }
    })
}

fn is_integer_type(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::SmallInt | DataType::Int | DataType::BigInt
    )
}

/// PostgreSQL types an integer literal as `int4` when it fits and `int8`
/// otherwise.
pub(crate) fn integer_literal_type(value: i64) -> DataType {
    if i32::try_from(value).is_ok() {
        DataType::Int
    } else {
        DataType::BigInt
    }
}

/// Returns the type of a column, cast, or literal expression without a
/// resolved source schema; other expressions return `None`.
pub(crate) fn known_expr_type(
    expr: &Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Option<DataType> {
    match expr {
        Expr::Column(name) => crate::sql::field_type_for_column(field_types, name).cloned(),
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::UnaryNumeric { expr, op } => {
            super::unary_numeric::result_type(*op, expr, known_expr_type(expr, field_types))
        }
        Expr::IntegerNegate { width, .. } => Some(width.data_type()),
        Expr::IntegerLiteral(value) => Some(integer_literal_type(*value)),
        Expr::RowCompare(_) | Expr::BoolLiteral(_) => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        _ => None,
    }
}

pub(super) fn data_type_for_parameter_oid(oid: i32) -> Option<DataType> {
    if let Some(element_oid) = u32::try_from(oid)
        .ok()
        .and_then(crate::catalog::builtin_identity::builtin_array_element_oid)
    {
        let element_oid = i32::try_from(element_oid).ok()?;
        let element_type = scalar_data_type_for_parameter_oid(element_oid)?;
        return Some(DataType::Array(Box::new(element_type)));
    }

    scalar_data_type_for_parameter_oid(oid)
}

fn scalar_data_type_for_parameter_oid(oid: i32) -> Option<DataType> {
    if let Some(metadata) = u32::try_from(oid)
        .ok()
        .and_then(crate::types::builtin::parameter_builtin_for_oid)
    {
        return Some(metadata.kind.data_type());
    }
    crate::types::schema::vector_dimensions_for_oid(i64::from(oid)).map(DataType::Vector)
}

fn preserve_alias_identity(source: &QuerySource, catalog: &Catalog, schema: &mut Schema) {
    if let QuerySource::Collection(name) = source {
        if catalog
            .get_schema(name)
            .is_some_and(|schema| !schema.declares_id())
        {
            if let Some(first) = schema.fields.first_mut() {
                first.name = "_id".into();
            }
        } else {
            schema.fields.push(FieldSchema {
                name: "_id".into(),
                data_type: DataType::Text,
                nullable: true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::infer_select_schema;
    use crate::catalog::Catalog;
    use crate::sql::QueryStatement;
    use crate::types::DataType;

    #[test]
    fn should_infer_qualified_wildcard_shape_from_unbound_select() {
        // Arrange
        let catalog = Catalog::new();
        catalog.register_collection(
            "inference_qualified_star_source",
            vec![
                ("id".to_owned(), DataType::BigInt),
                ("label".to_owned(), DataType::Text),
            ],
        );
        let parsed = crate::sql::parse_statement(
            "SELECT inference_qualified_star_source.* FROM inference_qualified_star_source",
        )
        .expect("qualified wildcard SQL parses");
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("qualified wildcard is a SELECT");
        };

        // Act
        let schema = infer_select_schema(&select, &catalog).expect("infer SELECT schema");

        // Assert
        assert_eq!(
            schema
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["id", "label"]
        );
        assert_eq!(schema.fields[0].data_type, DataType::BigInt);
        assert_eq!(schema.fields[1].data_type, DataType::Text);
    }
}
