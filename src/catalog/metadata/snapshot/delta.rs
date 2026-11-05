//! Compose a detached catalog with only the transaction's own published-map changes.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::exact_eq::ExactMetadataEq;
use super::heap::HeapBytes;
use super::{CaptureBoundary, Catalog, CatalogReadGuards, ControlledCatalogSnapshot};
use crate::app::CassieError;
use crate::executor::retained_memory::{add, hash_table_bytes};
use crate::runtime::QueryExecutionControls;

// This is the publication set of Catalog::publish_transaction_snapshot. All
// other maps inherit the supplied view, including live authorization metadata.
pub(super) fn is_transaction_map(name: &str) -> bool {
    matches!(
        name,
        "collections"
            | "namespaces"
            | "schemas"
            | "constraints"
            | "indexes"
            | "graphs"
            | "views"
            | "sequences"
            | "rollups"
            | "retention_policies"
            | "vector_indexes"
            | "projections"
            | "cardinality"
    )
}

impl Catalog {
    /// Inputs are stable under the caller's publication/session ownership.
    /// `self` chooses the initial SQL view or the current descriptor view.
    pub(crate) fn snapshot_with_delta(
        &self,
        base: &Self,
        overlay: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<ControlledCatalogSnapshot>, CassieError> {
        self.snapshot_with_delta_observer(base, overlay, controls, |_| {})
    }

    pub(super) fn snapshot_with_delta_observer(
        &self,
        base: &Self,
        overlay: &Self,
        controls: &QueryExecutionControls,
        mut observer: impl FnMut(CaptureBoundary),
    ) -> Result<Arc<ControlledCatalogSnapshot>, CassieError> {
        let (initial, base, staged) = loop {
            controls.check_interruption()?;
            if let (Some(initial), Some(base), Some(staged)) = (
                CatalogReadGuards::try_from(self),
                CatalogReadGuards::try_from(base),
                CatalogReadGuards::try_from(overlay),
            ) {
                break (initial, base, staged);
            }
            observer(CaptureBoundary::WaitingForLocks);
            controls.check_interruption()?;
            std::thread::yield_now();
        };
        let bytes = initial.delta_retained_bytes(&base, &staged, controls)?;
        let memory = controls.reserve_query_memory(bytes)?;
        observer(CaptureBoundary::BeforeClone);
        controls.check_interruption()?;
        let catalog = initial.clone_catalog_delta(
            &base,
            &staged,
            overlay.version(),
            controls,
            &mut observer,
        )?;
        let owner = Arc::new(ControlledCatalogSnapshot { catalog, memory });
        controls.check_interruption()?;
        Ok(owner)
    }

    /// Compare logical record content, including removals and unchanged
    /// dependencies, rather than capacities or the global version counter.
    pub(crate) fn matches_transaction_metadata(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        self.matches_transaction_metadata_observer(other, controls, |_| {})
    }

    pub(super) fn matches_transaction_metadata_observer(
        &self,
        other: &Self,
        controls: &QueryExecutionControls,
        mut observer: impl FnMut(CaptureBoundary),
    ) -> Result<bool, CassieError> {
        let (initial, current) = loop {
            controls.check_interruption()?;
            if let (Some(initial), Some(current)) = (
                CatalogReadGuards::try_from(self),
                CatalogReadGuards::try_from(other),
            ) {
                break (initial, current);
            }
            observer(CaptureBoundary::WaitingForLocks);
            controls.check_interruption()?;
            std::thread::yield_now();
        };
        initial.matches_transaction_maps(&current, controls)
    }
}

fn selected_value<'a, V: ExactMetadataEq>(
    initial: Option<&'a V>,
    base: Option<&V>,
    staged: Option<&'a V>,
    controls: &QueryExecutionControls,
) -> Result<Option<&'a V>, CassieError> {
    controls.check_interruption()?;
    match (base, staged) {
        (Some(_), None) => Ok(None),
        (None, Some(value)) => Ok(Some(value)),
        (Some(before), Some(after)) if !before.exact_eq(after, controls)? => Ok(Some(after)),
        _ => Ok(initial),
    }
}

fn visit_delta<V: ExactMetadataEq>(
    initial: &HashMap<String, V>,
    base: &HashMap<String, V>,
    staged: &HashMap<String, V>,
    controls: &QueryExecutionControls,
    mut visit: impl FnMut(&String, &V) -> Result<(), CassieError>,
) -> Result<(), CassieError> {
    for (key, value) in initial {
        if let Some(selected) =
            selected_value(Some(value), base.get(key), staged.get(key), controls)?
        {
            visit(key, selected)?;
        }
    }
    for (key, value) in staged {
        controls.check_interruption()?;
        if !initial.contains_key(key) {
            if let Some(selected) = selected_value(None, base.get(key), Some(value), controls)? {
                visit(key, selected)?;
            }
        }
    }
    Ok(())
}

pub(super) fn delta_map_bytes<V: HeapBytes + ExactMetadataEq>(
    initial: &HashMap<String, V>,
    base: &HashMap<String, V>,
    staged: &HashMap<String, V>,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    let mut entries = 0;
    let mut backing = 0;
    visit_delta(initial, base, staged, controls, |key, value| {
        entries = add(entries, 1)?;
        backing = add(
            add(backing, key.heap_bytes(controls)?)?,
            value.heap_bytes(controls)?,
        )?;
        Ok(())
    })?;
    add(hash_table_bytes::<(String, V)>(entries)?, backing)
}

pub(super) fn clone_delta_map<V: Clone + ExactMetadataEq>(
    initial: &HashMap<String, V>,
    base: &HashMap<String, V>,
    staged: &HashMap<String, V>,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(CaptureBoundary),
) -> Result<Arc<RwLock<HashMap<String, V>>>, CassieError> {
    let mut entries = 0;
    visit_delta(initial, base, staged, controls, |_, _| {
        entries = add(entries, 1)?;
        Ok(())
    })?;
    let mut target = HashMap::new();
    target.try_reserve(entries).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to allocate catalog delta map: {error}"))
    })?;
    visit_delta(initial, base, staged, controls, |key, value| {
        controls.check_interruption()?;
        target.insert(key.clone(), value.clone());
        Ok(())
    })?;
    observer(CaptureBoundary::MapCloned);
    controls.check_interruption()?;
    Ok(Arc::new(RwLock::new(target)))
}
