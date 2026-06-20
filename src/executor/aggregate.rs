use std::collections::{HashMap, HashSet};

use crate::catalog::{CollectionSchema, FunctionMeta};
use crate::executor::ColumnMeta;
use crate::sql::ast::SelectItem;
use crate::types::DataType;

pub fn columns_from_projection(
    projection: &[SelectItem],
    collection_schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Vec<ColumnMeta> {
    if projection.is_empty() {
        return vec![ColumnMeta::from_data_type("*", DataType::Text)];
    }

    projection
        .iter()
        .flat_map(|item| match item {
            SelectItem::Wildcard => {
                if let Some(collection_schema) = collection_schema {
                    let mut columns = Vec::with_capacity(collection_schema.fields.len() + 1);
                    let mut seen = HashSet::new();
                    let id = "id".to_string();
                    seen.insert(id.clone());
                    columns.push(ColumnMeta::from_data_type(id, DataType::Text));
                    for field in &collection_schema.fields {
                        if seen.insert(field.name.clone()) {
                            columns.push(ColumnMeta::from_data_type(
                                field.name.clone(),
                                field.data_type.clone(),
                            ));
                        }
                    }
                    columns.into_iter().collect()
                } else {
                    vec![ColumnMeta::from_data_type("*", DataType::Text)]
                }
            }
            SelectItem::Column { name, alias } => {
                let data_type = column_data_type(name, collection_schema);
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| name.clone()),
                    data_type,
                )]
            }
            SelectItem::Function { function, alias } => {
                let data_type =
                    function_return_type(&function.name, user_functions).unwrap_or(DataType::Text);
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| function.name.clone()),
                    data_type,
                )]
            }
            SelectItem::WindowFunction { function, alias } => vec![ColumnMeta::from_data_type(
                alias.clone().unwrap_or_else(|| function.name.clone()),
                DataType::BigInt,
            )],
        })
        .collect()
}

fn function_return_type(
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Option<DataType> {
    if let Some(metadata) = user_functions.get(&name.to_ascii_lowercase()) {
        return Some(metadata.return_type.clone());
    }

    match name.to_ascii_lowercase().as_str() {
        "count" => Some(DataType::Int),
        "sum" | "avg" => Some(DataType::Float),
        "min" | "max" => Some(DataType::Text),
        "search" | "search_score" | "vector_distance" | "vector_score" | "cosine_distance"
        | "dot_product" | "hybrid_score" => Some(DataType::Float),
        "snippet" | "version" | "current_schema" | "current_database" | "current_user"
        | "session_user" | "current_role" => Some(DataType::Text),
        "cast" => Some(DataType::Text),
        _ => None,
    }
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
        .map(|field| field.data_type.clone())
        .unwrap_or(DataType::Text)
}
