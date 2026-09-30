//! Static column-count check for set operations.
//!
//! `UNION`, `INTERSECT` and `EXCEPT` require every operand to produce the same
//! number of columns. PostgreSQL rejects a mismatch before execution, so the
//! check must not depend on either branch producing rows.

use super::wildcard::wildcard_output_fields;
use super::{CassieError, Catalog, CommonTableExpression, HashMap, SelectItem, SelectStatement};

/// Rejects a set operation whose left operand and right operand (the right
/// chain's leading SELECT) have different static output widths. A width that
/// cannot be resolved statically is left to the executor's row check.
///
/// # Errors
///
/// Returns a planner error naming both widths when they differ.
pub(super) fn validate_set_operand_widths(
    select: &SelectStatement,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let Some(set) = &select.set else {
        return Ok(());
    };
    let (Some(left), Some(right)) = (
        static_output_width(select, &[], catalog),
        static_output_width(&set.right, &select.ctes, catalog),
    ) else {
        return Ok(());
    };
    if left == right {
        Ok(())
    } else {
        Err(CassieError::Planner(format!(
            "set operation column count mismatch: {left} != {right}"
        )))
    }
}

fn static_output_width(
    select: &SelectStatement,
    outer_ctes: &[CommonTableExpression],
    catalog: &Catalog,
) -> Option<usize> {
    let mut width = 0usize;
    let mut wildcard_width = None;
    for item in &select.projection {
        match item {
            SelectItem::Wildcard => {
                if wildcard_width.is_none() {
                    let ctes: Vec<CommonTableExpression> =
                        select.ctes.iter().chain(outer_ctes).cloned().collect();
                    let fields =
                        wildcard_output_fields(&select.source, &ctes, catalog, &HashMap::new())
                            .ok()?;
                    wildcard_width = Some(fields.len());
                }
                width += wildcard_width?;
            }
            SelectItem::Column { name, .. } if name.ends_with('*') => return None,
            _ => width += 1,
        }
    }
    Some(width)
}
