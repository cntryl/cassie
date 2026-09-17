use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

use crate::catalog::{CollectionSchema, FunctionMeta};
use crate::executor::ColumnMeta;
use crate::sql::ast::SelectItem;
use crate::types::{DataType, FieldSchema, Schema};

#[must_use]
pub fn columns_from_projection<S: BuildHasher>(
    projection: &[SelectItem],
    collection_schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta, S>,
) -> Vec<ColumnMeta> {
    columns_from_projection_with_parameter_oids(projection, collection_schema, user_functions, &[])
}

#[must_use]
pub fn columns_from_projection_with_parameter_oids<S: BuildHasher>(
    projection: &[SelectItem],
    collection_schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta, S>,
    parameter_type_oids: &[i32],
) -> Vec<ColumnMeta> {
    if projection.is_empty() {
        return vec![ColumnMeta::from_data_type("*", &DataType::Text)];
    }

    let source_schema = projection_source_schema(collection_schema);
    let user_functions = user_functions
        .iter()
        .map(|(name, metadata)| (name.clone(), metadata.clone()))
        .collect::<HashMap<_, _>>();

    projection
        .iter()
        .flat_map(|item| match item {
            SelectItem::Wildcard => {
                if let Some(collection_schema) = collection_schema {
                    if crate::catalog::virtual_views::schema(&collection_schema.collection)
                        .is_some()
                    {
                        collection_schema
                            .fields
                            .iter()
                            .map(|field| {
                                ColumnMeta::from_data_type(field.name.clone(), &field.data_type)
                            })
                            .collect()
                    } else {
                        let mut columns = Vec::with_capacity(collection_schema.fields.len() + 1);
                        let mut seen = HashSet::new();
                        seen.insert("id".to_string());
                        columns.push(ColumnMeta::from_data_type("id", &DataType::Text));
                        for field in &collection_schema.fields {
                            if seen.insert(field.name.to_ascii_lowercase()) {
                                columns.push(ColumnMeta::from_data_type(
                                    field.name.clone(),
                                    &field.data_type,
                                ));
                            }
                        }
                        columns.into_iter().collect()
                    }
                } else {
                    vec![ColumnMeta::from_data_type("*", &DataType::Text)]
                }
            }
            SelectItem::Column { name, alias } => {
                let data_type = column_data_type(name, collection_schema);
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| name.clone()),
                    &data_type,
                )]
            }
            SelectItem::Function { function, alias } => {
                let data_type = if function.name.eq_ignore_ascii_case("count") {
                    Some(DataType::BigInt)
                } else {
                    crate::sql::binder::infer_function_return_type(
                        function,
                        &source_schema,
                        &user_functions,
                        parameter_type_oids,
                    )
                }
                .unwrap_or(DataType::Text);
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| function.name.clone()),
                    &data_type,
                )]
            }
            SelectItem::Expr { expr, alias } => {
                let data_type = crate::sql::binder::infer_expr_type(
                    expr,
                    &source_schema,
                    &user_functions,
                    parameter_type_oids,
                )
                .unwrap_or(DataType::Text);
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| "expr".to_string()),
                    &data_type,
                )]
            }
            SelectItem::WindowFunction { function, alias } => vec![ColumnMeta::from_data_type(
                alias.clone().unwrap_or_else(|| function.name.clone()),
                &DataType::BigInt,
            )],
        })
        .collect()
}

fn projection_source_schema(collection_schema: Option<&CollectionSchema>) -> Schema {
    let Some(collection_schema) = collection_schema else {
        return Schema { fields: Vec::new() };
    };

    let mut fields = Vec::with_capacity(collection_schema.fields.len() + 1);
    fields.push(FieldSchema {
        name: "id".to_string(),
        data_type: DataType::Text,
        nullable: true,
    });
    fields.extend(collection_schema.fields.iter().map(|field| FieldSchema {
        name: field.name.clone(),
        data_type: field.data_type.clone(),
        nullable: true,
    }));
    Schema { fields }
}

fn column_data_type(name: &str, schema: Option<&CollectionSchema>) -> DataType {
    if name.eq_ignore_ascii_case("id") || name.eq_ignore_ascii_case("_id") {
        return DataType::Text;
    }

    let Some(schema) = schema else {
        return DataType::Text;
    };

    schema
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(name))
        .map_or(DataType::Text, |field| field.data_type.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::columns_from_projection;
    use crate::sql::ast::{FunctionCall, SelectItem};
    use crate::types::DataType;

    #[test]
    fn should_report_count_results_as_bigint() {
        // Arrange
        let projection = vec![SelectItem::Function {
            function: FunctionCall {
                name: "count".to_string(),
                args: vec![],
            },
            alias: None,
        }];

        // Act
        let columns = columns_from_projection(&projection, None, &HashMap::new());

        // Assert
        assert_eq!(columns[0].type_oid, DataType::BigInt.type_oid());
    }
}
