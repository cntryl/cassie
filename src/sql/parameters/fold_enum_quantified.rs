//! Parameter identity for `enum op ANY|ALL (...)` (#827).
//!
//! PostgreSQL resolves `e = ANY($1)` with `$1` as the enum's array type and
//! `$1 = ANY(es)` with `$1` as the element enum. Without this an untyped
//! parameter beside a named enum operand stays unknown (`42P18`).

use super::{Fold, Frame, ParameterAdmissionError};
use crate::sql::ast;

const fn unknown(oid: i32) -> bool {
    matches!(oid, 0 | 705)
}

impl Fold<'_, '_> {
    fn enum_array_oid(&self, element_oid: i32) -> Option<i32> {
        let metadata = self
            .catalog
            .get_enum_type_by_oid(u32::try_from(element_oid).ok()?)?;
        i32::try_from(metadata.array_oid()).ok()
    }

    fn enum_element_oid(&self, array_oid: i32) -> Option<i32> {
        let metadata = self
            .catalog
            .get_enum_type_by_array_oid(u32::try_from(array_oid).ok()?)?;
        i32::try_from(metadata.type_oid()).ok()
    }

    /// Gives the untyped operand of a quantified comparison the enum identity
    /// its named enum counterpart implies.
    pub(super) fn enum_quantified(
        &mut self,
        op: &ast::BinaryOp,
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        if !matches!(op, ast::BinaryOp::Quantified { .. }) {
            return Ok(());
        }
        let Some((left, right)) = self
            .summaries
            .values
            .get(frame.start)
            .zip(self.summaries.values.get(frame.start + 1))
            .map(|(left, right)| (left.oid, right.oid))
        else {
            return Ok(());
        };
        if unknown(right) {
            if let Some(array_oid) = self.enum_array_oid(left) {
                self.occurrences.coerce(
                    &mut self.summaries.values[frame.start + 1],
                    array_oid,
                    true,
                )?;
            }
        } else if unknown(left) {
            if let Some(element_oid) = self.enum_element_oid(right) {
                self.occurrences.coerce(
                    &mut self.summaries.values[frame.start],
                    element_oid,
                    true,
                )?;
            }
        }
        Ok(())
    }
}
