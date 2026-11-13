//! Session-scoped parsing: enum names resolve against the catalog (#827).

use std::rc::Rc;

use super::{Cassie, CassieSession};
use crate::catalog::UserType;
use crate::sql::EnumResolver;
use crate::types::EnumRef;

impl Cassie {
    /// Resolver mapping raw type text to an enum visible to `session`; an enum
    /// created inside its open transaction lives in the catalog overlay until
    /// commit.
    pub(crate) fn enum_type_resolver(&self, session: Option<&CassieSession>) -> EnumResolver {
        let context = self.binding_context_for_session(session);
        let catalog = self.type_catalog(session);
        let database = context.database.clone();
        let search_order = context
            .relation_search_order()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Rc::new(move |raw: &str| -> Option<EnumRef> {
            let order = search_order.iter().map(String::as_str).collect::<Vec<_>>();
            match catalog.resolve_user_type(&database, &order, raw)? {
                UserType::Enum(metadata) => Some(metadata.descriptor()),
                _ => None,
            }
        })
    }
}
