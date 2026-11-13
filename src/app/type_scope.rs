//! Session-scoped parsing: composite type names resolve against the catalog.

use std::rc::Rc;

use super::{Cassie, CassieError, CassieSession};
use crate::sql::ast::ParsedStatement;
use crate::sql::{parse_statement, with_domain_scope, with_type_scope, TypeResolver};
use crate::types::CompositeRef;

impl Cassie {
    /// The catalog `session` reads types from: a type created inside its open
    /// transaction lives in the catalog overlay until commit, and a
    /// repeatable-read transaction reads its snapshot.
    pub(super) fn type_catalog(&self, session: Option<&CassieSession>) -> crate::catalog::Catalog {
        session
            .and_then(CassieSession::transaction_catalog_overlay)
            .or_else(|| {
                // REPEATABLE READ executes against its snapshot, so names must
                // resolve against the same descriptors.
                session
                    .and_then(CassieSession::transaction_catalog_snapshot)
                    .map(|snapshot| snapshot.catalog().clone())
            })
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
                // Every table owns a row type named like the table.
                .or_else(|| catalog.resolve_relation_row_type(&database, &order, raw))
        })
    }

    /// Parses `sql` with `session`'s composite type names in scope.
    pub(crate) fn parse_statement_for_session(
        &self,
        session: Option<&CassieSession>,
        sql: &str,
    ) -> Result<ParsedStatement, CassieError> {
        let catalog = self.type_catalog(session);
        if !catalog.has_composite_types()
            && !catalog.has_collections()
            && !catalog.has_domain_types()
        {
            return Ok(parse_statement(sql)?);
        }
        let resolver = self.composite_type_resolver(session);
        let domains = self.domain_type_resolver(session);
        Ok(with_domain_scope(domains, || {
            with_type_scope(resolver, || parse_statement(sql))
        })?)
    }

    /// Parses a data type name with `session`'s composite and domain type
    /// names in scope (declared PREPARE parameters, CTAS result columns).
    pub(crate) fn parse_data_type_for_session(
        &self,
        session: Option<&CassieSession>,
        raw: &str,
    ) -> Result<crate::types::DataType, String> {
        let resolver = self.composite_type_resolver(session);
        let domains = self.domain_type_resolver(session);
        with_domain_scope(domains, || {
            with_type_scope(resolver, || {
                crate::sql::parser::parse_data_type_in_scope(raw)
            })
        })
        .map_err(|error| error.to_string())
    }
}
