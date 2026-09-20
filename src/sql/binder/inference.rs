use super::{
    virtual_views, BinaryOp, CassieError, Catalog, CommonTableExpression, CteQuery, DataType, Expr,
    FieldSchema, FunctionCall, HashMap, QuerySource, QueryStatement, Schema, SelectItem,
    SelectStatement,
};
use crate::catalog::name_matches;

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn infer_select_schema(
    select: &SelectStatement,
    catalog: &Catalog,
) -> Result<Schema, CassieError> {
    let user_functions = catalog
        .list_functions()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function))
        .collect::<HashMap<_, _>>();

    infer_select_schema_with_scope(select, catalog, &HashMap::new(), &user_functions)
}

pub(super) fn infer_select_schema_with_scope(
    select: &SelectStatement,
    catalog: &Catalog,
    outer_ctes: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Schema, CassieError> {
    let mut cte_schemas = outer_ctes.clone();
    for cte in &select.ctes {
        let schema = infer_cte_schema(cte, catalog, &cte_schemas, user_functions)?;
        cte_schemas.insert(cte.name.to_ascii_lowercase(), schema);
    }

    let source_schema =
        infer_source_schema(&select.source, catalog, &cte_schemas, user_functions, false)?;
    let mut fields = infer_projection_schema(&select.projection, &source_schema, user_functions);

    if let Some(set) = &select.set {
        let right_schema =
            infer_select_schema_with_scope(&set.right, catalog, &cte_schemas, user_functions)?;
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
    let wanted = name.to_ascii_lowercase();
    let user_functions = catalog
        .list_functions()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function))
        .collect::<HashMap<_, _>>();

    let mut in_scope: HashMap<String, Schema> = HashMap::new();
    for cte in ctes {
        let schema = infer_cte_schema(cte, catalog, &in_scope, &user_functions).ok()?;
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
    let query = match &cte.query {
        CteQuery::Simple(statement) => statement,
        CteQuery::Recursive { base, .. } => base,
    };

    let QueryStatement::Select(select) = &query.statement else {
        return Err(CassieError::Planner(
            "CTE body must be a SELECT statement".into(),
        ));
    };

    let mut schema = infer_select_schema_with_scope(select, catalog, cte_schemas, user_functions)?;

    if !cte.aliases.is_empty() {
        if schema.fields.len() != cte.aliases.len() {
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

pub(super) fn infer_source_schema(
    source: &QuerySource,
    catalog: &Catalog,
    cte_schemas: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    qualify: bool,
) -> Result<Schema, CassieError> {
    let schema = match source {
        QuerySource::Collection(name) => relation_output_schema(catalog, name)?,
        QuerySource::Cte(name) => cte_schemas
            .get(&name.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?,
        QuerySource::SingleRow => Schema { fields: Vec::new() },
        QuerySource::TableFunction { name, .. } => {
            let schema = Schema {
                fields: super::select::table_function_columns(name)
                    .into_iter()
                    .map(|(name, data_type)| FieldSchema {
                        name,
                        data_type,
                        nullable: true,
                    })
                    .collect(),
            };
            qualify_schema(&schema, name)
        }
        QuerySource::Subquery { alias, select, .. } => {
            let inner =
                infer_select_schema_with_scope(select, catalog, cte_schemas, user_functions)?;
            qualify_schema(&inner, alias)
        }
        QuerySource::Join { left, right, .. } => {
            let left = infer_source_schema(left, catalog, cte_schemas, user_functions, true)?;
            let right = infer_source_schema(right, catalog, cte_schemas, user_functions, true)?;
            let mut fields = left.fields;
            fields.extend(right.fields);
            Schema { fields }
        }
    };

    if qualify {
        Ok(match source {
            QuerySource::Collection(name) | QuerySource::Cte(name) => qualify_schema(&schema, name),
            QuerySource::SingleRow
            | QuerySource::TableFunction { .. }
            | QuerySource::Subquery { .. }
            | QuerySource::Join { .. } => schema,
        })
    } else {
        Ok(schema)
    }
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

pub(super) fn infer_projection_schema(
    projection: &[SelectItem],
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Schema {
    let mut fields = Vec::new();
    for item in projection {
        match item {
            SelectItem::Wildcard => fields.extend(source_schema.fields.iter().cloned()),
            SelectItem::Column { name, alias } => {
                let output_name = alias.clone().unwrap_or_else(|| name.clone());
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
                        &[],
                    )
                    .unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::Expr { expr, alias } => {
                fields.push(FieldSchema {
                    name: alias.as_deref().unwrap_or("expr").to_string(),
                    data_type: infer_expr_type(expr, source_schema, user_functions, &[])
                        .unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::WindowFunction { function, alias } => {
                let data_type = match function.name.to_ascii_lowercase().as_str() {
                    "lag" | "lead" | "first_value" | "last_value" => function
                        .args
                        .first()
                        .and_then(|arg| infer_expr_type(arg, source_schema, user_functions, &[]))
                        .unwrap_or(DataType::Text),
                    _ => DataType::BigInt,
                };
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

pub(super) fn schema_field_type(schema: &Schema, name: &str) -> Option<DataType> {
    schema
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(name) || name_matches(&field.name, name))
        .map(|field| field.data_type.clone())
}

pub(crate) fn infer_function_return_type(
    function: &FunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let name = function.name.to_ascii_lowercase();
    if let Some(metadata) = user_functions.get(&name).or_else(|| {
        user_functions
            .values()
            .find(|metadata| name_matches(&metadata.name, &function.name))
    }) {
        return Some(metadata.return_type.clone());
    }

    let metadata = crate::sql::functions::function(&name)?;
    match metadata.return_type {
        crate::sql::functions::FunctionReturnType::Float => Some(DataType::Float),
        crate::sql::functions::FunctionReturnType::Text => Some(DataType::Text),
        crate::sql::functions::FunctionReturnType::Int => Some(DataType::Int),
        crate::sql::functions::FunctionReturnType::BigInt => Some(DataType::BigInt),
        crate::sql::functions::FunctionReturnType::Boolean => Some(DataType::Boolean),
        crate::sql::functions::FunctionReturnType::Timestamp => Some(DataType::Timestamp),
        crate::sql::functions::FunctionReturnType::FirstNonNullArgument => function
            .args
            .iter()
            .find_map(|arg| infer_expr_type(arg, source_schema, user_functions, parameter_types))
            .filter(|data_type| !matches!(data_type, DataType::Null))
            .or(Some(DataType::Text)),
        crate::sql::functions::FunctionReturnType::NumericArgument => function
            .args
            .first()
            .and_then(|expr| infer_expr_type(expr, source_schema, user_functions, parameter_types))
            .map(|data_type| match data_type {
                DataType::Int => DataType::Int,
                DataType::BigInt => DataType::BigInt,
                _ => DataType::Float,
            })
            .or(Some(DataType::Float)),
        crate::sql::functions::FunctionReturnType::SumArgument => function
            .args
            .first()
            .and_then(|expr| infer_expr_type(expr, source_schema, user_functions, parameter_types))
            .map(|data_type| match data_type {
                DataType::Int | DataType::SmallInt | DataType::BigInt => DataType::BigInt,
                _ => DataType::Float,
            })
            .or(Some(DataType::Float)),
        crate::sql::functions::FunctionReturnType::Unknown => None,
    }
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
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::IntegerLiteral(value) => Some(integer_literal_type(*value)),
        Expr::BoolLiteral(_)
        | Expr::Exists(_)
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Not { .. } => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        Expr::Param(index) => parameter_types
            .get(*index)
            .copied()
            .and_then(data_type_for_parameter_oid)
            .or(Some(DataType::Null)),
        Expr::Binary { left, op, right } => match op {
            BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
            | BinaryOp::Like => Some(DataType::Boolean),
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                let left_type =
                    infer_expr_type(left, source_schema, user_functions, parameter_types);
                let right_type =
                    infer_expr_type(right, source_schema, user_functions, parameter_types);
                if left_type.as_ref().is_some_and(is_integer_type)
                    && right_type.as_ref().is_some_and(is_integer_type)
                {
                    Some(DataType::BigInt)
                } else {
                    Some(DataType::Float)
                }
            }
            BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
                Some(DataType::Float)
            }
        },
    }
}

pub(super) fn common_case_type(left: DataType, right: DataType) -> Option<DataType> {
    if left == DataType::Null {
        return Some(right);
    }
    if right == DataType::Null {
        return Some(left);
    }
    if left == right {
        return Some(left);
    }
    if matches!(
        left,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    ) && matches!(
        right,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    ) {
        return Some(DataType::Text);
    }
    if matches!(
        left,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
    ) && matches!(
        right,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
    ) {
        if left == DataType::Float || right == DataType::Float {
            return Some(DataType::Float);
        }
        if left == DataType::BigInt || right == DataType::BigInt {
            return Some(DataType::BigInt);
        }
        if left == DataType::Int || right == DataType::Int {
            return Some(DataType::Int);
        }
        return Some(DataType::SmallInt);
    }
    None
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
        Expr::IntegerLiteral(value) => Some(integer_literal_type(*value)),
        Expr::BoolLiteral(_) => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        _ => None,
    }
}

fn data_type_for_parameter_oid(oid: i32) -> Option<DataType> {
    match oid {
        16 => Some(DataType::Boolean),
        17 => Some(DataType::Bytea),
        20 => Some(DataType::BigInt),
        21 => Some(DataType::SmallInt),
        23 => Some(DataType::Int),
        25 => Some(DataType::Text),
        114 => Some(DataType::Json),
        701 => Some(DataType::Float),
        1042 => Some(DataType::Char { length: None }),
        1043 => Some(DataType::Varchar { length: None }),
        1082 => Some(DataType::Date),
        1083 => Some(DataType::Time),
        1114 => Some(DataType::Timestamp),
        2950 => Some(DataType::Uuid),
        oid if oid > 33000 => usize::try_from(oid - 33000).ok().map(DataType::Vector),
        _ => None,
    }
}
