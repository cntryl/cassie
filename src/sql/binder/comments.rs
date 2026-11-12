//! Binds only persisted schema and physical table COMMENT targets.

use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::Catalog;
use crate::sql::ast::{
    CommentOnObjectKind, CommentOnStatement, IdentifierComponent, IdentifierPath,
};
use crate::sql::binder::{resolve_relation_name, BindingContext};

pub(super) fn bind_comment_on(
    mut statement: CommentOnStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CommentOnStatement, CassieError> {
    match statement.kind {
        CommentOnObjectKind::Schema => {
            let schema =
                super::context::resolve_schema_name(statement.object.as_str(), catalog, context)?;
            if !catalog.namespace_exists(&schema) && !crate::catalog::is_system_schema(&schema) {
                return Err(CassieError::CatalogObjectNotFound {
                    kind: CatalogObjectKind::Schema,
                    name: schema,
                });
            }
            statement.object = parse_canonical_path(&schema)?;
        }
        CommentOnObjectKind::Table | CommentOnObjectKind::Column => {
            let relation = resolve_relation_name(statement.object.as_str(), catalog, context)?;
            if catalog.get_collection(&relation).is_none() {
                if catalog.get_view(&relation).is_some()
                    || catalog.get_materialized_projection(&relation).is_some()
                {
                    return Err(CassieError::Unsupported(
                        "comments on views are not supported".to_owned(),
                    ));
                }
                return Err(CassieError::CatalogObjectNotFound {
                    kind: CatalogObjectKind::Relation,
                    name: relation,
                });
            }
            if statement.kind == CommentOnObjectKind::Column {
                let column = statement.column.as_ref().ok_or_else(|| {
                    CassieError::Planner("COMMENT ON COLUMN requires a column name".to_owned())
                })?;
                let schema = catalog.get_schema(&relation).ok_or_else(|| {
                    CassieError::CatalogObjectNotFound {
                        kind: CatalogObjectKind::Relation,
                        name: relation.clone(),
                    }
                })?;
                let stored_column = schema
                    .fields
                    .iter()
                    .find(|field| identifier_matches_field(column, &field.name))
                    .map(|field| field.name.clone())
                    .ok_or_else(|| CassieError::CatalogObjectNotFound {
                        kind: CatalogObjectKind::Relation,
                        name: format!("{}.{}", relation, column.value()),
                    })?;
                statement.column = Some(IdentifierComponent::delimited(stored_column));
            }
            statement.object = parse_canonical_path(&relation)?;
        }
    }
    Ok(statement)
}

fn parse_canonical_path(name: &str) -> Result<IdentifierPath, CassieError> {
    IdentifierPath::parse(name).map_err(CassieError::Planner)
}

fn identifier_matches_field(identifier: &IdentifierComponent, stored: &str) -> bool {
    if identifier.is_delimited() {
        stored == identifier.value()
    } else {
        stored == identifier.value().to_ascii_lowercase()
    }
}
