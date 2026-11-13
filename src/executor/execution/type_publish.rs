//! Publication of an `ALTER TYPE` (or row-type refresh) result (#828).
//!
//! Each Cassie database is one Midge column family and a Midge transaction
//! spans one family, so everything that lives in the type's own database is
//! one atomic transaction. Dependents in other databases are published
//! afterwards, one transaction per database. The altered type is always
//! persisted first, so an interruption can only leave a dependent on an older
//! revision; hydration and the failure path roll those forward.
use std::collections::BTreeMap;

use crate::app::Cassie;
use crate::catalog::composite_alter::{CompositeGroup, DescriptorPropagation};
use crate::catalog::CompositeTypeMeta;
use crate::executor::QueryError;

/// The altered type and, for a rename, the name it replaces.
pub(super) struct Altered<'a> {
    pub next: &'a CompositeTypeMeta,
    pub previous_name: Option<&'a str>,
}

fn group_for<'a>(
    groups: &'a mut BTreeMap<String, CompositeGroup>,
    database: &str,
) -> &'a mut CompositeGroup {
    groups
        .entry(database.to_owned())
        .or_insert_with(|| CompositeGroup {
            database: database.to_owned(),
            ..CompositeGroup::default()
        })
}

fn groups(
    cassie: &Cassie,
    altered: Option<&Altered<'_>>,
    propagation: DescriptorPropagation,
) -> Vec<CompositeGroup> {
    let mut by_database = BTreeMap::new();
    let target_database = altered.map(|altered| altered.next.database.clone());
    if let Some(altered) = altered {
        let group = group_for(&mut by_database, &altered.next.database);
        group.target = Some(altered.next.clone());
        group.deleted_type = altered.previous_name.map(str::to_owned);
    }
    for dependent in propagation.types {
        group_for(&mut by_database, &dependent.database.clone())
            .types
            .push(dependent);
    }
    for column in propagation.columns {
        let database = cassie.midge.relation_database(&column.table);
        group_for(&mut by_database, &database).columns.push(column);
    }
    for view in propagation.views {
        let database = cassie.midge.relation_database(&view.name);
        group_for(&mut by_database, &database).views.push(view);
    }
    let mut ordered = Vec::with_capacity(by_database.len());
    if let Some(first) = target_database.and_then(|database| by_database.remove(&database)) {
        ordered.push(first);
    }
    ordered.extend(by_database.into_values());
    ordered
}

fn apply_to_catalog(cassie: &Cassie, group: CompositeGroup) {
    for column in group.columns {
        cassie
            .catalog
            .set_collection_field_type(&column.table, &column.column, column.data_type);
    }
    for view in group.views {
        cassie.catalog.register_view(view);
    }
    for dependent in group.types {
        cassie.catalog.register_composite_type(dependent);
    }
    if let Some(old) = &group.deleted_type {
        cassie.catalog.unregister_composite_type(old);
    }
    if let Some(target) = group.target {
        cassie.catalog.register_composite_type(target);
    }
}

/// Publishes the altered type and its dependents.
///
/// # Errors
///
/// Returns the storage error of the first failing group. A failure in the
/// first (type) group changes nothing; a later failure leaves the type
/// published and the catalog rolled forward to it.
pub(super) fn publish_change(
    cassie: &Cassie,
    altered: Option<&Altered<'_>>,
    propagation: DescriptorPropagation,
) -> Result<(), QueryError> {
    let mut published_type = false;
    for group in groups(cassie, altered, propagation) {
        let carries_type = group.target.is_some();
        if let Err(error) = cassie.midge.publish_composite_group(&group) {
            if published_type || altered.is_none() {
                cassie.catalog.reconcile_embedded_composites();
            }
            return Err(QueryError::from(error));
        }
        apply_to_catalog(cassie, group);
        published_type |= carries_type;
    }
    Ok(())
}
