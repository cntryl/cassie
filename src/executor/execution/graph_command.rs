use super::{Cassie, DataType, QueryError, FieldSchema, Schema, catalog};

pub(super) fn create_graph_collection(
    cassie: &Cassie,
    collection: &str,
    builtin_fields: Vec<(String, DataType)>,
    user_fields: &[crate::sql::ast::FieldDefinition],
) -> Result<(), QueryError> {
    let mut schema_fields = builtin_fields
        .into_iter()
        .map(|(name, data_type)| FieldSchema {
            name,
            data_type,
            nullable: true,
        })
        .collect::<Vec<_>>();
    schema_fields.extend(user_fields.iter().map(|field| FieldSchema {
        name: field.name.clone(),
        data_type: field.data_type.clone(),
        nullable: true,
    }));

    let schema = Schema {
        fields: schema_fields,
    };
    let metadata = catalog::CollectionMeta::new(collection, None);
    cassie
        .midge
        .create_collection_with_meta(collection, schema.clone(), metadata.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    let constraints = user_fields
        .iter()
        .flat_map(|field| field.constraints.iter().cloned())
        .collect::<Vec<_>>();
    cassie
        .midge
        .save_constraints(collection, constraints.as_slice())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_collection_meta_with_constraints(
        metadata,
        schema
            .fields
            .into_iter()
            .map(|field| (field.name, field.data_type))
            .collect(),
        constraints,
    );
    Ok(())
}
