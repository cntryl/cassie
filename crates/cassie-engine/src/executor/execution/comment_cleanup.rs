use super::{Cassie, QueryError};

pub(in crate::executor::execution) fn cleanup_relation_comments(
    cassie: &Cassie,
    relation: &str,
    relation_id: u64,
) -> Result<(), QueryError> {
    let database = crate::catalog::relation_database_name(relation)
        .unwrap_or_else(|| cassie.default_database.clone());
    let object_oid = crate::catalog::object_identity::normal_oid(relation_id).ok_or_else(|| {
        QueryError::Cassie(crate::app::CassieError::Unsupported(
            "comment cleanup requires a stable table catalog identity".to_owned(),
        ))
    })?;
    cassie
        .midge
        .delete_object_comments_for_object(&database, 1259, object_oid)
        .map_err(QueryError::Cassie)?;
    cassie
        .metadata_write_catalog
        .unregister_relation_comments(&database, object_oid);
    Ok(())
}

pub(in crate::executor::execution) fn cleanup_schema_comment(
    cassie: &Cassie,
    database: &str,
    schema: &str,
) -> Result<(), QueryError> {
    let object_oid = crate::catalog::object_identity::stored_namespace_oid(&cassie.catalog, schema)
        .ok_or_else(|| {
            QueryError::Cassie(crate::app::CassieError::Unsupported(
                "comment cleanup requires a stable schema catalog identity".to_owned(),
            ))
        })?;
    let address = crate::catalog::ObjectCommentAddress {
        database: database.to_ascii_lowercase(),
        class_oid: 2615,
        object_oid,
        object_sub_id: 0,
    };
    cassie
        .midge
        .delete_object_comment(&address)
        .map_err(QueryError::Cassie)?;
    cassie
        .metadata_write_catalog
        .unregister_object_comment(&address);
    Ok(())
}
