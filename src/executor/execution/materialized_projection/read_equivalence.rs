use std::collections::BTreeSet;

use super::{plan_projection_query, projection_binding_context, Cassie, HashMap};
use crate::executor::execution::{QuerySource, SelectItem};

pub(in crate::executor::execution) fn preserves_base_columns(
    cassie: &Cassie,
    projection_name: &str,
    query: &str,
    source: &str,
    needed: &BTreeSet<String>,
) -> bool {
    let context = projection_binding_context(cassie, None, Some(projection_name));
    let Ok(built) = plan_projection_query(cassie, query, &HashMap::new(), &context) else {
        return false;
    };
    let plan = built.logical;
    if !matches!(&plan.source, QuerySource::Collection(collection) if collection.as_str() == source)
        || plan.filter.is_some()
        || plan.limit_value().is_some()
        || plan.offset_value().is_some()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.ctes.is_empty()
    {
        return false;
    }
    let identity_columns = plan
        .projection
        .iter()
        .filter_map(|item| {
            let SelectItem::Column { name, alias } = item else {
                return None;
            };
            if crate::types::row_identity::is_row_identity_column(name)
                || alias.as_ref().is_some_and(|alias| {
                    !crate::sql::ColumnIdentifierPath::matches_stored_field(name, alias)
                })
            {
                return None;
            }
            Some(crate::sql::ColumnIdentifierPath::reference_field_key(name))
        })
        .collect::<BTreeSet<_>>();
    needed.iter().all(|name| identity_columns.contains(name))
}
