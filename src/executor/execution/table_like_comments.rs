//! `CREATE TABLE ... (LIKE source INCLUDING COMMENTS)` column comment copy (#826).
use super::{Cassie, QueryError};
use crate::catalog::{
    object_identity::stored_relation_oid, ObjectCommentAddress, ObjectCommentMeta,
};

const PG_CLASS_OID: u32 = 1259;

/// Copies each column comment of `source` to the same-named column of `target`.
///
/// Cassie supports comments on schemas, tables and columns only, so there are
/// no constraint or index comments to copy; the table comment itself is not
/// copied, matching PostgreSQL.
pub(super) fn copy_column_comments(
    cassie: &Cassie,
    source: &str,
    target: &str,
) -> Result<(), QueryError> {
    let (Some(source_oid), Some(target_oid)) = (
        stored_relation_oid(&cassie.catalog, source),
        stored_relation_oid(&cassie.catalog, target),
    ) else {
        return Ok(());
    };
    let source_database = crate::catalog::relation_database_name(source)
        .unwrap_or_else(|| cassie.default_database.clone())
        .to_ascii_lowercase();
    let target_database = crate::catalog::relation_database_name(target)
        .unwrap_or_else(|| cassie.default_database.clone())
        .to_ascii_lowercase();
    let source_schema = cassie
        .midge
        .row_schema(source)
        .map_err(QueryError::Cassie)?;
    let target_schema = cassie
        .midge
        .row_schema(target)
        .map_err(QueryError::Cassie)?;
    for field in source_schema.fields.iter().filter(|field| !field.retired) {
        let Ok(source_sub_id) = i32::try_from(field.field_id) else {
            continue;
        };
        let Some(description) = cassie.catalog.object_comment(
            &source_database,
            PG_CLASS_OID,
            source_oid,
            source_sub_id,
        ) else {
            continue;
        };
        let Some(target_sub_id) = target_schema
            .fields
            .iter()
            .find(|candidate| !candidate.retired && candidate.name == field.name)
            .and_then(|candidate| i32::try_from(candidate.field_id).ok())
        else {
            continue;
        };
        let metadata = ObjectCommentMeta {
            format_version: ObjectCommentMeta::FORMAT_VERSION,
            address: ObjectCommentAddress {
                database: target_database.clone(),
                class_oid: PG_CLASS_OID,
                object_oid: target_oid,
                object_sub_id: target_sub_id,
            },
            description,
        };
        cassie
            .midge
            .put_object_comment(&metadata)
            .map_err(QueryError::Cassie)?;
        cassie
            .metadata_write_catalog
            .register_object_comment(metadata);
    }
    Ok(())
}
