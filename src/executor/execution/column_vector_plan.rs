//! The EXPLAIN side of the column vector decision (#1358).
//!
//! A statement reads vector batches when the typed operator that serves it
//! opens an authoritative column source. This module asks the same questions
//! the typed projected read and the typed aggregates ask before they open a
//! scan, without opening one, and applies the shared type eligibility to the
//! fields they would read. The scan dispatch applies the same type rule when it
//! opens its source, so plan text and execution agree.

use super::{
    aggregate_exec, projected_read, scan, virtual_views, Cassie, LogicalPlan,
    QueryExecutionControls, Value,
};
use crate::executor::typed_batch::column_vector_eligibility::type_eligibility;
use crate::executor::typed_batch::ColumnVectorEligibility;
use crate::executor::QueryError;
use crate::sql::ast::{QuerySource, SelectItem};
use crate::types::row_identity::is_row_identity_column;

/// Whether `plan` would read vector batches from its column-authoritative table.
///
/// # Errors
///
/// Returns resource-limit and cancellation errors from the planning reads.
pub(crate) fn column_vector_eligibility(
    cassie: &Cassie,
    plan: &LogicalPlan,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<ColumnVectorEligibility, QueryError> {
    if plan.command.is_some() {
        return Ok(ColumnVectorEligibility::NotApplicable);
    }
    if matches!(plan.source, QuerySource::Join { .. }) {
        let mut leaves = Vec::new();
        collection_leaves(&plan.source, &mut leaves);
        return Ok(
            if leaves
                .iter()
                .any(|leaf| is_column_authoritative(cassie, leaf))
            {
                ColumnVectorEligibility::join()
            } else {
                ColumnVectorEligibility::NotApplicable
            },
        );
    }
    let Some(collection) = crate::sql::physical_collection(&plan.source) else {
        return Ok(ColumnVectorEligibility::NotApplicable);
    };
    if virtual_views::schema(collection).is_some()
        || cassie.catalog.get_view(collection).is_some()
        || !is_column_authoritative(cassie, collection)
    {
        return Ok(ColumnVectorEligibility::NotApplicable);
    }
    let (schema, _schema_memory) = cassie
        .catalog
        .clone_schema_with_controls(collection, controls)?;
    let Some(schema) = schema else {
        return Ok(ColumnVectorEligibility::NotApplicable);
    };
    let aggregates = !plan.group_by.is_empty()
        || plan
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::Function { .. }));
    let fields = if aggregates {
        match aggregate_exec::typed::explain_shape(plan, &schema, params, controls)? {
            aggregate_exec::typed::PlanShape::Fields(fields) => fields,
            aggregate_exec::typed::PlanShape::Aggregate => {
                return Ok(ColumnVectorEligibility::aggregate_shape());
            }
            aggregate_exec::typed::PlanShape::Expression => {
                return Ok(ColumnVectorEligibility::expression());
            }
        }
    } else {
        match projected_read::explain_shape(cassie, plan, params, collection, controls)? {
            projected_read::ProjectedShape::Fields(fields) => fields,
            projected_read::ProjectedShape::Expression => {
                return Ok(ColumnVectorEligibility::expression());
            }
            projected_read::ProjectedShape::TransportBudget => {
                return Ok(ColumnVectorEligibility::transport_budget());
            }
            projected_read::ProjectedShape::CoveringIndex => {
                return Ok(ColumnVectorEligibility::covering_index());
            }
        }
    };
    let mut types = Vec::with_capacity(fields.len());
    for field in fields.iter().filter(|field| !is_row_identity_column(field)) {
        let Some(data_type) = scan::field_data_type(Some(&schema), field) else {
            return Ok(ColumnVectorEligibility::expression());
        };
        types.push(data_type);
    }
    Ok(type_eligibility(types))
}

/// The base collections a join reads.
fn collection_leaves<'a>(source: &'a QuerySource, leaves: &mut Vec<&'a str>) {
    match source {
        QuerySource::Join { left, right, .. } => {
            collection_leaves(left, leaves);
            collection_leaves(right, leaves);
        }
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            collection_leaves(source, leaves);
        }
        other => leaves.extend(crate::sql::physical_collection(other)),
    }
}

fn is_column_authoritative(cassie: &Cassie, collection: &str) -> bool {
    cassie
        .catalog
        .collection_storage_mode(collection)
        .is_some_and(crate::catalog::collections::CollectionStorageMode::uses_column_store_storage)
}
