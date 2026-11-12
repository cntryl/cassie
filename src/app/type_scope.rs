//! Session-scoped parsing: composite type names resolve against the catalog.

use std::rc::Rc;

use super::{Cassie, CassieError, CassieSession};
use crate::sql::ast::ParsedStatement;
use crate::sql::{parse_statement, with_type_scope, TypeResolver};
use crate::types::CompositeRef;

impl Cassie {
    /// The catalog `session` reads types from: a type created inside its open
    /// transaction lives in the catalog overlay until commit.
    fn type_catalog(&self, session: Option<&CassieSession>) -> crate::catalog::Catalog {
        session
            .and_then(CassieSession::transaction_catalog_overlay)
            .unwrap_or_else(|| self.catalog.clone())
    }

    /// Resolver mapping raw type text to a composite type visible to `session`.
    pub(crate) fn composite_type_resolver(&self, session: Option<&CassieSession>) -> TypeResolver {
        let context = self.binding_context_for_session(session);
        let catalog = self.type_catalog(session);
        let database = context.database.clone();
        let search_order = context
            .relation_search_order()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Rc::new(move |raw: &str| -> Option<CompositeRef> {
            let order = search_order.iter().map(String::as_str).collect::<Vec<_>>();
            catalog
                .resolve_composite_type(&database, &order, raw)
                .map(|metadata| metadata.descriptor())
        })
    }

    /// Parses `sql` with `session`'s composite type names in scope.
    pub(crate) fn parse_statement_for_session(
        &self,
        session: Option<&CassieSession>,
        sql: &str,
    ) -> Result<ParsedStatement, CassieError> {
        if !self.type_catalog(session).has_composite_types() {
            return Ok(parse_statement(sql)?);
        }
        let resolver = self.composite_type_resolver(session);
        Ok(with_type_scope(resolver, || parse_statement(sql))?)
    }
}
