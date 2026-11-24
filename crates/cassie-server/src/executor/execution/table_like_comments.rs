//! Prepare selected LIKE column comments before atomic target publication.
use super::{Cassie, QueryError};
use crate::catalog::object_identity::stored_relation_oid;

/// Snapshot source column comments under the caller's source collection gate.
/// The adapter validates and persists them in the target creation transaction.
pub(super) fn prepare_column_comments(
    cassie: &Cassie,
    source: &str,
) -> Result<Vec<(String, String)>, QueryError> {
    let Some(source_oid) = stored_relation_oid(&cassie.catalog, source) else {
        return Ok(Vec::new());
    };
    let database = crate::catalog::relation_database_name(source)
        .unwrap_or_else(|| cassie.default_database.clone());
    let schema = cassie.midge.row_schema(source)?;
    let mut comments = Vec::new();
    for field in schema.fields.iter().filter(|field| !field.retired) {
        let Ok(sub_id) = i32::try_from(field.field_id) else {
            continue;
        };
        if let Some(description) = cassie
            .catalog
            .object_comment(&database, 1259, source_oid, sub_id)
        {
            comments.push((field.name.clone(), description));
        }
    }
    Ok(comments)
}
