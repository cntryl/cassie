use super::{
    is_row_id_column, projected_scan_fields, source_contains_join, BTreeSet, IndexKind, IndexMeta,
    LogicalPlan, QuerySource,
};

pub(super) fn column_batch_index(plan: &LogicalPlan, indexes: &[IndexMeta]) -> Option<String> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || source_contains_join(&plan.source)
    {
        return None;
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let fields = projected_scan_fields(plan)?;
    let needed = fields
        .into_iter()
        .filter(|field| !is_row_id_column(field))
        .map(|field| crate::sql::ColumnIdentifierPath::reference_field_key(&field))
        .collect::<BTreeSet<_>>();
    if needed.is_empty() {
        return None;
    }
    indexes
        .iter()
        .filter(|index| *collection == index.collection && index.kind == IndexKind::Column)
        .find(|index| {
            let available = index
                .normalized_fields()
                .into_iter()
                .map(|field| crate::sql::ColumnIdentifierPath::stored_field_key(&field))
                .collect::<BTreeSet<_>>();
            needed.is_subset(&available)
        })
        .map(|index| index.name.clone())
}
