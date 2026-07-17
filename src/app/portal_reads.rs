use super::{Cassie, CassieError, CassieSession};
use crate::sql::ast::{QuerySource, SelectItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortalReadSpec {
    pub(crate) collection: String,
    pub(crate) source_fields: Vec<String>,
    pub(crate) includes_wildcard: bool,
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
            collection: collection.clone(),
            source_fields,
            includes_wildcard,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_resolve_portal_collection_with_session_search_path() {
        // Arrange
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
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
}
