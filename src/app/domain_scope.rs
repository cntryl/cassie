//! Session-scoped parsing: domain names resolve against the catalog (#828).

use std::rc::Rc;

use super::{Cassie, CassieSession};
use crate::sql::DomainResolver;
use crate::types::DomainRef;

impl Cassie {
    /// Resolver mapping raw type text to a domain visible to `session`; a
    /// domain created inside its open transaction lives in the catalog overlay
    /// until commit.
    pub(crate) fn domain_type_resolver(&self, session: Option<&CassieSession>) -> DomainResolver {
        let context = self.binding_context_for_session(session);
        let catalog = session
            .and_then(CassieSession::transaction_catalog_overlay)
            .unwrap_or_else(|| self.catalog.clone());
        let database = context.database.clone();
        let search_order = context
            .relation_search_order()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Rc::new(move |raw: &str| -> Option<DomainRef> {
            let order = search_order.iter().map(String::as_str).collect::<Vec<_>>();
            catalog
                .resolve_domain_type(&database, &order, raw)
                .map(|metadata| metadata.descriptor())
        })
    }
}
