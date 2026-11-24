//! Carry admitted scalar/ARRAY subquery outputs without a second catalog walk.

use super::{Fold, Frame, Node, Summary};
use crate::catalog::builtin_identity::{builtin_array_element_oid, builtin_array_oid};
use crate::sql::ast::{
    Expr, QueryStatement, SelectItem, SelectStatement, SubqueryExpr, SubqueryKind,
};

impl Fold<'_, '_> {
    pub(super) fn subquery_select_summary(
        &self,
        select: &SelectStatement,
        context: usize,
        target_start: usize,
    ) -> Option<Summary> {
        if !matches!(
            self.frames.values.last()?.node,
            Node::Query(QueryStatement::Select(_))
        ) {
            return None;
        }
        let parent = self
            .frames
            .values
            .get(self.frames.values.len().checked_sub(2)?)?;
        if !scalar_or_array(parent.node) || select.set.is_some() || select.projection.len() != 1 {
            return None;
        }
        let oid = match select.projection.first()? {
            SelectItem::Column { name, .. } => self
                .column_oid(context, name)
                .or_else(|| self.column_oid(parent.context, name))?,
            SelectItem::Expr { .. } | SelectItem::Function { .. } => {
                let summary = self.targets.values.get(target_start)?.summary;
                if summary.literal && matches!(summary.oid, 0 | 705) {
                    25
                } else {
                    summary.oid
                }
            }
            SelectItem::Wildcard | SelectItem::WindowFunction { .. } => return None,
        };
        // A subquery is an expression result, not a parameter occurrence to
        // retype from the caller after its own target has been finalized.
        Some(Summary::known(oid))
    }

    pub(super) fn subquery_query_summary(&self, frame: &Frame<'_>) -> Option<Summary> {
        scalar_or_array(self.frames.values.last()?.node)
            .then(|| self.summaries.values.get(frame.start).copied())
            .flatten()
    }

    pub(super) fn subquery_output_summary(
        &self,
        subquery: &SubqueryExpr,
        frame: &Frame<'_>,
    ) -> Option<Summary> {
        if let Some(kind) = subquery.result_type() {
            return i32::try_from(kind.type_oid()).ok().map(Summary::known);
        }
        if !matches!(subquery.kind, SubqueryKind::Scalar | SubqueryKind::Array) {
            return None;
        }
        let mut oid = u32::try_from(self.summaries.values.get(frame.start)?.oid).ok()?;
        if matches!(subquery.kind, SubqueryKind::Array) && builtin_array_element_oid(oid).is_none()
        {
            oid = builtin_array_oid(oid)?;
        }
        i32::try_from(oid).ok().map(Summary::known)
    }
}

fn scalar_or_array(node: Node<'_>) -> bool {
    matches!(node, Node::Expr(Expr::Subquery(subquery))
        if matches!(subquery.kind, SubqueryKind::Scalar | SubqueryKind::Array))
}
