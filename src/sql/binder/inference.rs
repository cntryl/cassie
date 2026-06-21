use super::*;

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
            field.name = alias.clone();
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
            QuerySource::SingleRow | QuerySource::Subquery { .. } | QuerySource::Join { .. } => {
                schema
            }
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

    let schema = catalog
        .get_schema(name)
        .ok_or_else(|| CassieError::CollectionNotFound(name.to_string()))?;

    let mut fields = Vec::with_capacity(schema.fields.len() + 1);
    fields.push(FieldSchema {
        name: "id".to_string(),
        data_type: DataType::Text,
        nullable: true,
    });
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
                    data_type: infer_function_return_type(function, source_schema, user_functions)
                        .unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::Expr { alias, .. } => {
                fields.push(FieldSchema {
                    name: alias.as_deref().unwrap_or("expr").to_string(),
                    data_type: DataType::Float,
                    nullable: true,
                });
            }
            SelectItem::WindowFunction { function, alias } => {
                fields.push(FieldSchema {
                    name: alias
                        .as_deref()
                        .unwrap_or(function.name.as_str())
                        .to_string(),
                    data_type: DataType::BigInt,
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
        .find(|field| field.name.eq_ignore_ascii_case(name))
        .map(|field| field.data_type.clone())
}

pub(super) fn infer_function_return_type(
    function: &FunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Option<DataType> {
    let name = function.name.to_ascii_lowercase();
    if let Some(metadata) = user_functions.get(&name) {
        return Some(metadata.return_type.clone());
    }

    match name.as_str() {
        "count" => Some(DataType::Int),
        "sum" | "avg" => Some(DataType::Float),
        "min" | "max" => Some(DataType::Text),
        "length" | "len" => Some(DataType::Int),
        "lower" | "upper" | "substring" | "trim" | "concat" => Some(DataType::Text),
        "coalesce" => function
            .args
            .iter()
            .find_map(|arg| infer_expr_type(arg, source_schema))
            .filter(|data_type| !matches!(data_type, DataType::Null))
            .or(Some(DataType::Text)),
        "abs" => function
            .args
            .first()
            .and_then(|expr| infer_expr_type(expr, source_schema))
            .map(|data_type| match data_type {
                DataType::Int => DataType::Int,
                DataType::Float => DataType::Float,
                _ => DataType::Float,
            })
            .or(Some(DataType::Float)),
        "search" | "search_score" | "vector_distance" | "vector_score" | "cosine_distance"
        | "dot_product" | "hybrid_score" => Some(DataType::Float),
        "snippet" | "version" | "current_schema" | "current_database" | "current_user"
        | "session_user" | "current_role" => Some(DataType::Text),
        _ => None,
    }
}

pub(super) fn infer_expr_type(expr: &Expr, source_schema: &Schema) -> Option<DataType> {
    match expr {
        Expr::Column(name) => schema_field_type(source_schema, name),
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::BoolLiteral(_) => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        _ => None,
    }
}
