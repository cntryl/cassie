//! Result column metadata for set operations.
//!
//! A set operation's output column takes the common type of every operand's
//! column at that position (PostgreSQL's UNION type resolution), not the left
//! operand's type: `SELECT int_col ... UNION SELECT float_col ...` is `float8`.
//! The binder rejects operand pairs with no common type, so only numeric
//! widening and character-type unification happen here.

use super::plan_inspection::logical_plan_from_select;
use super::{aggregate, catalog, FunctionMeta, HashMap};
use crate::executor::ColumnMeta;
use crate::planner::logical::LogicalPlan;
use crate::types::DataType;

/// Returns `columns` (the left operand's result columns) with each column's
/// type replaced by the common type across every set-operation operand.
#[must_use]
pub(crate) fn unify_set_result_columns(
    catalog: &catalog::Catalog,
    logical: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    parameter_type_oids: &[i32],
    columns: Vec<ColumnMeta>,
) -> Vec<ColumnMeta> {
    let Some(set) = &logical.set else {
        return columns;
    };
    let mut right_plan = logical_plan_from_select(&set.right);
    // The right operand reads CTEs declared on the whole statement.
    let mut ctes = logical.ctes.clone();
    ctes.extend(right_plan.ctes);
    right_plan.ctes = ctes;
    let right_columns = operand_columns(catalog, &right_plan, user_functions, parameter_type_oids);
    let right_columns = unify_set_result_columns(
        catalog,
        &right_plan,
        user_functions,
        parameter_type_oids,
        right_columns,
    );
    if right_columns.len() != columns.len() {
        return columns;
    }
    columns
        .into_iter()
        .zip(right_columns)
        .map(|(left, right)| unify_column(left, &right))
        .collect()
}

fn operand_columns(
    catalog: &catalog::Catalog,
    logical: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    parameter_type_oids: &[i32],
) -> Vec<ColumnMeta> {
    let collection_schema = catalog
        .get_schema(&logical.collection)
        .or_else(|| catalog::CollectionSchema::virtual_view(&logical.collection))
        .or_else(|| {
            crate::sql::binder::cte_collection_schema_with_functions(
                &logical.ctes,
                &logical.collection,
                catalog,
                user_functions,
            )
        });
    let wildcard_fields = aggregate::wildcard_fields_for_plan(catalog, logical, user_functions);
    aggregate::columns_from_projection_with_wildcard(
        &logical.projection,
        collection_schema.as_ref(),
        wildcard_fields.as_deref(),
        user_functions,
        parameter_type_oids,
    )
}

fn unify_column(left: ColumnMeta, right: &ColumnMeta) -> ColumnMeta {
    if left.type_oid == right.type_oid || right.type_oid == UNKNOWN_OID {
        return left;
    }
    if left.type_oid == UNKNOWN_OID {
        return ColumnMeta {
            name: left.name,
            ..right.clone()
        };
    }
    let common = match (numeric_rank(left.type_oid), numeric_rank(right.type_oid)) {
        (Some(left_rank), Some(right_rank)) => {
            if right_rank > left_rank {
                numeric_type(right.type_oid)
            } else {
                return left;
            }
        }
        _ if is_character(left.type_oid) && is_character(right.type_oid) => Some(DataType::Text),
        _ => None,
    };
    match common {
        Some(data_type) => ColumnMeta {
            nullable: left.nullable || right.nullable,
            ..ColumnMeta::from_data_type(left.name, &data_type)
        },
        None => left,
    }
}

const UNKNOWN_OID: i64 = 705;

const fn numeric_rank(type_oid: i64) -> Option<u8> {
    match type_oid {
        21 => Some(1),
        23 => Some(2),
        20 => Some(3),
        701 => Some(4),
        _ => None,
    }
}

const fn numeric_type(type_oid: i64) -> Option<DataType> {
    match type_oid {
        21 => Some(DataType::SmallInt),
        23 => Some(DataType::Int),
        20 => Some(DataType::BigInt),
        701 => Some(DataType::Float),
        _ => None,
    }
}

const fn is_character(type_oid: i64) -> bool {
    matches!(type_oid, 25 | 1042 | 1043)
}
