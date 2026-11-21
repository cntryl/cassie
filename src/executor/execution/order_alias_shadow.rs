//! Detects an ORDER BY name that an output alias takes away from a source column.
//!
//! `SELECT b AS a FROM t ORDER BY a` orders by the output column `a`, which is source `b`. The
//! access paths read a bare ORDER BY name as a source field, so when the name is both an output
//! alias of another column and a source column, they would order by the wrong one. The generic
//! sort resolves aliases first, so such plans skip the access paths.

use crate::planner::logical::LogicalPlan;
use crate::sql::ast::Expr;
use crate::sql::ColumnIdentifierPath;

/// Whether any ORDER BY item names an output alias of a different column while a source column
/// of the same name exists. An unknown source schema counts as shadowed.
pub(super) fn order_alias_shadows_source_column(
    plan: &LogicalPlan,
    source_fields: Option<&[String]>,
) -> bool {
    plan.order.iter().any(|order| {
        let Expr::Column(reference) = &order.expr else {
            return false;
        };
        let Some(Expr::Column(target)) =
            crate::executor::sort::alias_expr(&order.expr, &plan.projection)
        else {
            return false;
        };
        let reference_key = ColumnIdentifierPath::reference_field_key(reference);
        if ColumnIdentifierPath::reference_field_key(&target) == reference_key {
            return false;
        }
        source_fields.is_none_or(|fields| {
            fields
                .iter()
                .any(|field| ColumnIdentifierPath::stored_field_key(field) == reference_key)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::order_alias_shadows_source_column;
    use crate::planner::logical::LogicalPlan;

    fn plan(sql: &str) -> LogicalPlan {
        let parsed = crate::sql::parse_statement(sql).expect("parse statement");
        super::super::build_logical_plan(&crate::catalog::Catalog::new(), &parsed)
            .expect("build logical plan")
    }

    fn fields() -> Vec<String> {
        vec!["a".to_string(), "b".to_string()]
    }

    #[test]
    fn should_report_an_alias_that_renames_another_column_to_a_source_name() {
        // Arrange
        let plan = plan("SELECT b AS a FROM t ORDER BY a");

        // Act
        let shadowed = order_alias_shadows_source_column(&plan, Some(&fields()));

        // Assert
        assert!(shadowed);
    }

    #[test]
    fn should_not_report_an_alias_whose_name_is_not_a_source_column() {
        // Arrange
        let plan = plan("SELECT b AS renamed FROM t ORDER BY renamed");

        // Act
        let shadowed = order_alias_shadows_source_column(&plan, Some(&fields()));

        // Assert
        assert!(!shadowed);
    }

    #[test]
    fn should_not_report_a_column_aliased_to_its_own_name() {
        // Arrange
        let plan = plan("SELECT a AS a, b FROM t ORDER BY a");

        // Act
        let shadowed = order_alias_shadows_source_column(&plan, Some(&fields()));

        // Assert
        assert!(!shadowed);
    }

    #[test]
    fn should_report_a_renaming_alias_when_the_source_schema_is_unknown() {
        // Arrange
        let plan = plan("SELECT b AS a FROM t ORDER BY a");

        // Act
        let shadowed = order_alias_shadows_source_column(&plan, None);

        // Assert
        assert!(shadowed);
    }
}
