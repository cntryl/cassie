//! Authorization of `GRAPH_TABLE` sources (#1256).

use super::{CassieError, CassieSession, RoleObjectPrivilege};
use crate::sql::ast::GraphTable;

impl crate::app::Cassie {
    /// A graph grants nothing: reading it needs `SELECT` on every table the pattern can
    /// read, exactly as querying those tables directly would. An unknown graph or label is
    /// left to binding to report.
    pub(super) fn authorize_graph_table(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        table: &GraphTable,
    ) -> Result<(), CassieError> {
        if let Ok(graph) = crate::sql::binder::resolve_graph(&self.catalog, context, &table.graph) {
            if let Ok(tables) = crate::sql::binder::graph_table_tables_read(&graph, table) {
                for mapped in tables {
                    // A canonical name is database-qualified, which an unscoped context
                    // refuses; the schema-qualified form resolves under either.
                    let reference = match crate::catalog::parse_name(&mapped) {
                        Ok(
                            crate::catalog::ParsedName::DatabaseQualified { schema, name, .. }
                            | crate::catalog::ParsedName::SchemaQualified { schema, name },
                        ) => format!(
                            "{}.{}",
                            crate::catalog::object_identity::quote_identifier(&schema),
                            crate::catalog::object_identity::quote_identifier(&name)
                        ),
                        _ => mapped,
                    };
                    self.require_table_privilege(
                        session,
                        context,
                        &reference,
                        RoleObjectPrivilege::Select,
                    )?;
                }
            }
        }
        for expression in table.expressions() {
            self.authorize_expression(session, context, expression)?;
        }
        Ok(())
    }
}
