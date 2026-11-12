//! Persistence for the selected schema, table, and column COMMENT commands.

use super::command_output::empty as empty_command;
use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::catalog::{
    object_identity::{stored_namespace_oid, stored_relation_oid},
    ObjectCommentAddress, ObjectCommentMeta,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{CommentOnObjectKind, CommentOnStatement};

const PG_CLASS_OID: u32 = 1259;
const PG_NAMESPACE_OID: u32 = 2615;

pub(super) fn execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &CommentOnStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let address = object_address(cassie, session, statement)?;
    let output = empty_command("COMMENT", controls)?;
    let description = statement
        .description
        .as_deref()
        .filter(|description| !description.is_empty());

    if let Some(description) = description {
        let metadata = ObjectCommentMeta {
            format_version: ObjectCommentMeta::FORMAT_VERSION,
            address,
            description: description.to_owned(),
        };
        cassie
            .midge
            .put_object_comment(&metadata)
            .map_err(QueryError::Cassie)?;
        cassie
            .metadata_write_catalog
            .register_object_comment(metadata);
    } else {
        cassie
            .midge
            .delete_object_comment(&address)
            .map_err(QueryError::Cassie)?;
        cassie
            .metadata_write_catalog
            .unregister_object_comment(&address);
    }

    Ok(output)
}

fn object_address(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &CommentOnStatement,
) -> Result<ObjectCommentAddress, QueryError> {
    let database = session
        .and_then(CassieSession::current_database)
        .unwrap_or(&cassie.default_database)
        .to_ascii_lowercase();
    let (class_oid, object_oid, object_sub_id) = match statement.kind {
        CommentOnObjectKind::Schema => {
            let schema = statement.object.as_str();
            let target_database = crate::catalog::schema_database_name(schema)
                .unwrap_or_else(|| cassie.default_database.clone());
            require_current_database(&database, &target_database)?;
            if cassie.catalog.get_namespace(schema).is_none() {
                return Err(QueryError::Cassie(
                    crate::app::CassieError::CatalogObjectNotFound {
                        kind: crate::app::CatalogObjectKind::Schema,
                        name: schema.to_owned(),
                    },
                ));
            }
            let object_oid = stored_namespace_oid(&cassie.catalog, schema).ok_or_else(|| {
                unsupported("schema has no stable PostgreSQL catalog identity")
            })?;
            (PG_NAMESPACE_OID, object_oid, 0)
        }
        CommentOnObjectKind::Table | CommentOnObjectKind::Column => {
            let relation = statement.object.as_str();
            let collection = cassie.catalog.get_collection(relation).ok_or_else(|| {
                QueryError::Cassie(crate::app::CassieError::CatalogObjectNotFound {
                    kind: crate::app::CatalogObjectKind::Relation,
                    name: relation.to_owned(),
                })
            })?;
            let target_database = crate::catalog::relation_database_name(&collection.name)
                .unwrap_or_else(|| cassie.default_database.clone());
            require_current_database(&database, &target_database)?;
            let object_oid = stored_relation_oid(&cassie.catalog, &collection.name)
                .ok_or_else(|| unsupported("table has no stable PostgreSQL catalog identity"))?;
            let object_sub_id = if statement.kind == CommentOnObjectKind::Column {
                let column = statement.column.as_ref().ok_or_else(|| {
                    QueryError::General("COMMENT ON COLUMN requires a column name".to_owned())
                })?;
                let row_schema = cassie
                    .midge
                    .row_schema(&collection.name)
                    .map_err(QueryError::Cassie)?;
                let field_id = row_schema
                    .fields
                    .iter()
                    .find(|field| !field.retired && field.name == column.value())
                    .map(|field| field.field_id)
                    .ok_or_else(|| {
                        QueryError::Cassie(crate::app::CassieError::CatalogObjectNotFound {
                            kind: crate::app::CatalogObjectKind::Relation,
                            name: format!("{}.{}", relation, column.value()),
                        })
                    })?;
                i32::try_from(field_id)
                    .map_err(|_| unsupported("column attribute number exceeds PostgreSQL int4"))?
            } else {
                0
            };
            (PG_CLASS_OID, object_oid, object_sub_id)
        }
    };

    Ok(ObjectCommentAddress {
        database,
        class_oid,
        object_oid,
        object_sub_id,
    })
}

fn require_current_database(active: &str, target: &str) -> Result<(), QueryError> {
    if active.eq_ignore_ascii_case(target) {
        Ok(())
    } else {
        Err(unsupported(
            "comments on objects outside the current database are not supported",
        ))
    }
}

fn unsupported(message: &str) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::Unsupported(message.to_owned()))
}
