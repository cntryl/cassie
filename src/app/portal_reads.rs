use super::{Cassie, CassieError, CassieSession};
use crate::sql::ast::{QuerySource, SelectItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortalReadSpec {
    pub(crate) collection: String,
    pub(crate) source_fields: Vec<String>,
    pub(crate) includes_wildcard: bool,
    pub(crate) traversal: crate::midge::adapter::RowTraversal,
}

impl Cassie {
    pub(crate) fn resolve_portal_read_spec(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
    ) -> Result<Option<PortalReadSpec>, CassieError> {
        let controls = self.runtime.query_controls(std::time::Instant::now());
        let physical = self.compile_physical_plan(parsed, Some(session), Some(&controls))?;
        let QuerySource::Collection(collection) = &physical.logical.source else {
            return Ok(None);
        };
        let mut source_fields = Vec::new();
        let mut includes_wildcard = false;
        for item in &physical.logical.projection {
            match item {
                SelectItem::Wildcard => includes_wildcard = true,
                SelectItem::Column { name, .. }
                | SelectItem::Expr {
                    expr: crate::sql::ast::Expr::Column(name),
                    ..
                } => source_fields.push(name.clone()),
                _ => return Ok(None),
            }
        }
        Ok(Some(PortalReadSpec {
            collection: collection.to_string(),
            source_fields,
            includes_wildcard,
            traversal: if physical.logical.limit_value().is_some() {
                crate::midge::adapter::RowTraversal::VisitedRows
            } else {
                crate::midge::adapter::RowTraversal::CompleteSource
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_resolve_portal_collection_with_session_search_path() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie-portal-read-spec-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA portal_scope", vec![])
            .expect("create schema");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_scope.items (payload TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(&session, "SET search_path TO portal_scope", vec![])
            .expect("set search path");
        let parsed = crate::sql::parser::parse_statement("SELECT payload FROM items")
            .expect("parse statement");

        // Act
        let spec = cassie
            .resolve_portal_read_spec(&session, parsed)
            .expect("resolve portal read")
            .expect("streamable spec");

        // Assert
        assert!(spec.collection.ends_with(".portal_scope.items"));
        assert_eq!(spec.source_fields, ["payload"]);

        let _ = std::fs::remove_dir_all(path);
    }
    #[test]
    fn should_choose_portal_validation_scope_from_the_sql_source_limit() {
        // Arrange
        let cassie =
            Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
                .expect("memory Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_columns (label TEXT) USING columnar",
                vec![],
            )
            .expect("column table");

        // Act
        let full = cassie
            .resolve_portal_read_spec(
                &session,
                crate::sql::parse_statement("SELECT label FROM portal_columns")
                    .expect("full query"),
            )
            .expect("full spec")
            .expect("cursor");
        let bounded = cassie
            .resolve_portal_read_spec(
                &session,
                crate::sql::parse_statement("SELECT label FROM portal_columns LIMIT 1")
                    .expect("bounded query"),
            )
            .expect("bounded spec")
            .expect("cursor");

        // Assert
        assert_eq!(
            full.traversal,
            crate::midge::adapter::RowTraversal::CompleteSource
        );
        assert_eq!(
            bounded.traversal,
            crate::midge::adapter::RowTraversal::VisitedRows
        );
    }
}
