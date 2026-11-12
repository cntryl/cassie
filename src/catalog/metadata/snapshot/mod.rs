//! Retained SQL catalog rows and their transaction-owned query-memory lease.

use std::alloc::Layout;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use parking_lot::{RwLock, RwLockReadGuard};

use crate::app::CassieError;
use crate::executor::retained_memory::add;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::Catalog;

mod heap;
use heap::HeapBytes;
mod delta;
mod exact_eq;
use delta::{clone_delta_map, delta_map_bytes, is_transaction_map};
use exact_eq::ExactMetadataEq;

fn sequence_definition_exact_eq(
    left: &crate::catalog::SequenceMeta,
    right: &crate::catalog::SequenceMeta,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    Ok(left.name.exact_eq(&right.name, controls)?
        && left.data_type.exact_eq(&right.data_type, controls)?
        && left.start_value.exact_eq(&right.start_value, controls)?
        && left.increment_by.exact_eq(&right.increment_by, controls)?
        && left.min_value.exact_eq(&right.min_value, controls)?
        && left.max_value.exact_eq(&right.max_value, controls)?
        && left.cache.exact_eq(&right.cache, controls)?
        && left.cycle.exact_eq(&right.cycle, controls)?
        && left.owned_by.exact_eq(&right.owned_by, controls)?
        && left.persistence.exact_eq(&right.persistence, controls)?
        && left.object_id.exact_eq(&right.object_id, controls)?)
}

fn sequence_definitions_exact_eq(
    left: &HashMap<String, crate::catalog::SequenceMeta>,
    right: &HashMap<String, crate::catalog::SequenceMeta>,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (index, (name, sequence)) in left.iter().enumerate() {
        if index % 128 == 0 {
            controls.check_interruption()?;
        }
        let Some(other) = right.get(name) else {
            return Ok(false);
        };
        if !sequence_definition_exact_eq(sequence, other, controls)? {
            return Ok(false);
        }
    }
    controls.check_interruption()?;
    Ok(true)
}

// Exhaustive destructuring makes an added owned metadata field a compilation
// error until its retained backing is explicitly included in the admission.
macro_rules! metadata_heap {
    ($kind:path { $($field:ident),* $(,)? }) => {
        impl HeapBytes for $kind {
            fn heap_bytes(
                &self,
                controls: &crate::runtime::QueryExecutionControls,
            ) -> Result<usize, crate::app::CassieError> {
                controls.check_interruption()?;
                let Self { $($field),* } = self;
                let mut bytes = 0;
                $(bytes = crate::executor::retained_memory::add(
                    bytes, $field.heap_bytes(controls)?,
                )?;)*
                Ok(bytes)
            }
        }
        impl crate::catalog::metadata::snapshot::exact_eq::ExactMetadataEq for $kind {
            fn exact_eq(
                &self,
                other: &Self,
                controls: &crate::runtime::QueryExecutionControls,
            ) -> Result<bool, crate::app::CassieError> {
                controls.check_interruption()?;
                $(if !crate::catalog::metadata::snapshot::exact_eq::ExactMetadataEq::exact_eq(
                    &self.$field, &other.$field, controls,
                )? { return Ok(false); })*
                Ok(true)
            }
        }
    };
}

macro_rules! unit_enum_heap {
    ($kind:path { $($variant:ident),* $(,)? }) => {
        impl HeapBytes for $kind {
            fn heap_bytes(
                &self,
                controls: &crate::runtime::QueryExecutionControls,
            ) -> Result<usize, crate::app::CassieError> {
                controls.check_interruption()?;
                match self { $(Self::$variant)|* => Ok(0) }
            }
        }
        impl crate::catalog::metadata::snapshot::exact_eq::ExactMetadataEq for $kind {
            fn exact_eq(
                &self,
                other: &Self,
                controls: &crate::runtime::QueryExecutionControls,
            ) -> Result<bool, crate::app::CassieError> {
                controls.check_interruption()?;
                Ok(std::mem::discriminant(self) == std::mem::discriminant(other))
            }
        }
    };
}

mod metadata_heap;
mod vector_heap;

metadata_heap!(crate::types::Schema { fields });
metadata_heap!(crate::types::FieldSchema {
    name,
    data_type,
    nullable
});

pub(crate) struct ControlledCatalogSnapshot {
    catalog: Catalog,
    memory: QueryMemoryReservation,
}

impl ControlledCatalogSnapshot {
    pub(crate) const fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub(crate) const fn accounted_bytes(&self) -> usize {
        self.memory.bytes()
    }
}

impl fmt::Debug for ControlledCatalogSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledCatalogSnapshot")
            .field("version", &self.catalog.version())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureBoundary {
    WaitingForLocks,
    BeforeClone,
    MapCloned,
}

impl Catalog {
    /// Creates a private catalog view for one explicit transaction.
    #[must_use]
    pub(crate) fn detached_transaction_snapshot(&self) -> Self {
        Self {
            databases: Arc::new(RwLock::new(self.databases.read().clone())),
            collections: Arc::new(RwLock::new(self.collections.read().clone())),
            namespaces: Arc::new(RwLock::new(self.namespaces.read().clone())),
            schemas: Arc::new(RwLock::new(self.schemas.read().clone())),
            projections: Arc::new(RwLock::new(self.projections.read().clone())),
            constraints: Arc::new(RwLock::new(self.constraints.read().clone())),
            indexes: Arc::new(RwLock::new(self.indexes.read().clone())),
            graphs: Arc::new(RwLock::new(self.graphs.read().clone())),
            functions: Arc::new(RwLock::new(self.functions.read().clone())),
            procedures: Arc::new(RwLock::new(self.procedures.read().clone())),
            extensions: Arc::new(RwLock::new(self.extensions.read().clone())),
            views: Arc::new(RwLock::new(self.views.read().clone())),
            roles: Arc::new(RwLock::new(self.roles.read().clone())),
            role_object_grants: Arc::new(RwLock::new(self.role_object_grants.read().clone())),
            sequences: Arc::new(RwLock::new(self.sequences.read().clone())),
            rollups: Arc::new(RwLock::new(self.rollups.read().clone())),
            retention_policies: Arc::new(RwLock::new(self.retention_policies.read().clone())),
            vector_indexes: Arc::new(RwLock::new(self.vector_indexes.read().clone())),
            cardinality: Arc::new(RwLock::new(self.cardinality.read().clone())),
            projection_comparison_reports: Arc::new(RwLock::new(
                self.projection_comparison_reports.read().clone(),
            )),
            projection_consistency_reports: Arc::new(RwLock::new(
                self.projection_consistency_reports.read().clone(),
            )),
            projection_repair_reports: Arc::new(RwLock::new(
                self.projection_repair_reports.read().clone(),
            )),
            operational_assignments: Arc::new(RwLock::new(
                self.operational_assignments.read().clone(),
            )),
            maintenance_debts: Arc::new(RwLock::new(self.maintenance_debts.read().clone())),
            object_comments: Arc::new(RwLock::new(self.object_comments.read().clone())),
            role_object_grants: Arc::new(RwLock::new(
                self.role_object_grants.read().clone(),
            )),
            version: Arc::new(AtomicU64::new(self.version())),
        }
    }

    /// The caller holds the catalog publication write guard before entering its
    /// normal query-bound read scope. Names, descriptors and authorization remain
    /// live; this detached owner supplies the transaction's SQL catalog rows.
    pub(crate) fn snapshot_with_controls(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<ControlledCatalogSnapshot>, CassieError> {
        self.snapshot_with_observer(controls, |_| {})
    }

    fn snapshot_with_observer(
        &self,
        controls: &QueryExecutionControls,
        mut observer: impl FnMut(CaptureBoundary),
    ) -> Result<Arc<ControlledCatalogSnapshot>, CassieError> {
        let guards = loop {
            controls.check_interruption()?;
            if let Some(guards) = CatalogReadGuards::try_from(self) {
                break guards;
            }
            // Never hold a subset while blocking on another map: existing
            // catalog writers can acquire multiple maps in different orders.
            observer(CaptureBoundary::WaitingForLocks);
            controls.check_interruption()?;
            std::thread::yield_now();
        };
        let bytes = guards.retained_bytes(controls)?;
        let memory = controls.reserve_query_memory(bytes)?;
        observer(CaptureBoundary::BeforeClone);
        controls.check_interruption()?;
        let catalog = guards.clone_catalog(self.version(), controls, &mut observer)?;
        let owner = Arc::new(ControlledCatalogSnapshot { catalog, memory });
        controls.check_interruption()?;
        Ok(owner)
    }
}

fn arc_bytes<T>() -> Result<usize, CassieError> {
    Layout::new::<[usize; 2]>()
        .extend(Layout::new::<T>())
        .map(|(layout, _)| layout.pad_to_align().size())
        .map_err(|_| CassieError::ResourceLimit("catalog snapshot layout overflow".to_owned()))
}

fn clone_map<K: Clone + Eq + Hash, V: Clone>(
    source: &HashMap<K, V>,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(CaptureBoundary),
) -> Result<Arc<RwLock<HashMap<K, V>>>, CassieError> {
    controls.check_interruption()?;
    let mut target = HashMap::with_capacity(source.capacity());
    for (key, value) in source {
        controls.check_interruption()?;
        target.insert(key.clone(), value.clone());
    }
    observer(CaptureBoundary::MapCloned);
    controls.check_interruption()?;
    Ok(Arc::new(RwLock::new(target)))
}

macro_rules! catalog_maps {
    ($($field:ident: $value:ty),* $(,)?) => {
        struct CatalogReadGuards<'a> {
            $($field: RwLockReadGuard<'a, HashMap<String, $value>>),*
        }

        impl<'a> CatalogReadGuards<'a> {
            fn try_from(catalog: &'a Catalog) -> Option<Self> {
                let Catalog { $($field),*, version: _ } = catalog;
                Some(Self { $($field: $field.try_read()?),* })
            }

            fn retained_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
                let mut bytes = add(
                    arc_bytes::<ControlledCatalogSnapshot>()?,
                    arc_bytes::<AtomicU64>()?,
                )?;
                $(
                    bytes = add(bytes, arc_bytes::<RwLock<HashMap<String, $value>>>()?)?;
                    bytes = add(bytes, self.$field.heap_bytes(controls)?)?;
                )*
                Ok(bytes)
            }

            fn clone_catalog(
                &self,
                version: u64,
                controls: &QueryExecutionControls,
                observer: &mut impl FnMut(CaptureBoundary),
            ) -> Result<Catalog, CassieError> {
                Ok(Catalog {
                    $($field: clone_map(&self.$field, controls, observer)?),*,
                    version: Arc::new(AtomicU64::new(version)),
                })
            }

            fn delta_retained_bytes(
                &self,
                base: &Self,
                staged: &Self,
                controls: &QueryExecutionControls,
            ) -> Result<usize, CassieError> {
                let mut bytes = add(
                    arc_bytes::<ControlledCatalogSnapshot>()?,
                    arc_bytes::<AtomicU64>()?,
                )?;
                $(
                    bytes = add(bytes, arc_bytes::<RwLock<HashMap<String, $value>>>()?)?;
                    bytes = add(bytes, if is_transaction_map(stringify!($field)) {
                        delta_map_bytes(&self.$field, &base.$field, &staged.$field, controls)?
                    } else {
                        self.$field.heap_bytes(controls)?
                    })?;
                )*
                Ok(bytes)
            }

            fn clone_catalog_delta(
                &self,
                base: &Self,
                staged: &Self,
                version: u64,
                controls: &QueryExecutionControls,
                observer: &mut impl FnMut(CaptureBoundary),
            ) -> Result<Catalog, CassieError> {
                Ok(Catalog {
                    $($field: if is_transaction_map(stringify!($field)) {
                        clone_delta_map(&self.$field, &base.$field, &staged.$field, controls, observer)?
                    } else {
                        clone_map(&self.$field, controls, observer)?
                    }),*,
                    version: Arc::new(AtomicU64::new(version)),
                })
            }

            fn matches_transaction_maps(
                &self,
                other: &Self,
                controls: &QueryExecutionControls,
            ) -> Result<bool, CassieError> {
                $(
                    if is_transaction_map(stringify!($field)) {
                        let equal = if stringify!($field) == "sequences" {
                            sequence_definitions_exact_eq(
                                &self.sequences,
                                &other.sequences,
                                controls,
                            )?
                        } else {
                            self.$field.exact_eq(&other.$field, controls)?
                        };
                        if !equal { return Ok(false); }
                    }
                )*
                controls.check_interruption()?;
                Ok(true)
            }
        }
    };
}

catalog_maps! {
    databases: crate::catalog::DatabaseMeta,
    collections: crate::catalog::CollectionMeta,
    namespaces: crate::catalog::NamespaceMeta,
    schemas: crate::catalog::CollectionSchema,
    projections: crate::catalog::ProjectionMeta,
    constraints: Vec<crate::catalog::FieldConstraint>,
    indexes: crate::catalog::IndexMeta,
    graphs: crate::catalog::GraphMeta,
    functions: crate::catalog::FunctionMeta,
    procedures: crate::catalog::ProcedureMeta,
    extensions: crate::catalog::ExtensionMeta,
    views: crate::catalog::ViewMeta,
    roles: crate::catalog::RoleMeta,
    role_object_grants: crate::catalog::RoleObjectGrantMeta,
    sequences: crate::catalog::SequenceMeta,
    rollups: crate::catalog::RollupMeta,
    retention_policies: crate::catalog::RetentionPolicyMeta,
    vector_indexes: crate::embeddings::VectorIndexRecord,
    cardinality: crate::catalog::CollectionCardinalityStats,
    projection_comparison_reports: crate::catalog::ProjectionComparisonReportMeta,
    projection_consistency_reports: crate::catalog::ProjectionConsistencyReportMeta,
    projection_repair_reports: crate::catalog::ProjectionRepairReportMeta,
    operational_assignments: crate::catalog::OperationalAssignmentMeta,
    maintenance_debts: crate::catalog::MaintenanceDebtMeta,
    object_comments: crate::catalog::ObjectCommentMeta,
    role_object_grants: crate::catalog::RoleObjectGrantMeta,
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod delta_tests;
