//! Source-local OID lookup without adopting another namespace's leaf type.

use super::expressions::oid;
use super::Fold;
use crate::sql::{ast, binder, ColumnIdentifierPath};

impl Fold<'_, '_> {
    pub(super) fn column_oid(&self, context: usize, name: &str) -> Option<i32> {
        // Identifier projections and canonical qualifier variants remain transient
        // metadata-input work. Retain no parsed path, schema or namespace directory.
        let column = ColumnIdentifierPath::parse(name).ok()?;
        let fields = self.fields(context);
        let namespace = column.namespace_key();
        let lookup = column.lookup_key();
        if let Some(data_type) = fields.get(&namespace).or_else(|| fields.get(&lookup)) {
            return Some(oid(data_type));
        }
        let field = column.field_lookup_key();
        if !column.is_qualified() {
            return fields.get(&field).map(oid);
        }
        let qualifier = lookup.strip_suffix(&field)?.strip_suffix('.')?;
        let namespace = namespace.strip_suffix(&field)?.strip_suffix('.')?;
        let own = match self.contexts.values[context].source {
            ast::QuerySource::Collection(name) => crate::catalog::qualifier_variants(name)
                .iter()
                .any(|candidate| candidate == qualifier),
            ast::QuerySource::Cte(name) | ast::QuerySource::Subquery { alias: name, .. } => {
                owns_alias(name, namespace)
            }
            // Aliases expose their exact visible/physical keys above. A join's
            // bare map can belong to another relation, so never borrow its leaf.
            ast::QuerySource::Aliased { .. }
            | ast::QuerySource::Join { .. }
            | ast::QuerySource::TableFunction { .. }
            | ast::QuerySource::Values { .. }
            | ast::QuerySource::SingleRow => false,
        };
        if own {
            fields.get(&field).map(oid)
        } else {
            None
        }
    }

    // Borrow operand type identities without cloning an owned DataType.
    // Column and simple CASE must share the same namespace boundary.
    pub(super) fn known_operand_oid(&self, expr: &ast::Expr, context: usize) -> Option<i32> {
        let mut operand = expr;
        for _ in 0..=128 {
            if let ast::Expr::UnaryNumeric { expr, op } = operand {
                if binder::unary_numeric::is_unknown_operand(expr) {
                    return (*op == ast::UnaryNumericOp::Plus).then_some(701);
                }
                operand = expr;
            } else {
                break;
            }
        }
        match operand {
            ast::Expr::Column(name) => self.column_oid(context, name),
            ast::Expr::IntegerNegate { width, .. } => Some(oid(&width.data_type())),
            ast::Expr::Cast { data_type, .. } => Some(oid(data_type)),
            ast::Expr::StringLiteral(_) => Some(25),
            ast::Expr::NumberLiteral(_) => Some(701),
            ast::Expr::IntegerLiteral(value) => Some(oid(&binder::integer_literal_type(*value))),
            ast::Expr::RowCompare(_) | ast::Expr::BoolLiteral(_) => Some(16),
            ast::Expr::Null => Some(oid(&crate::types::DataType::Null)),
            ast::Expr::UnaryNumeric { .. }
            | ast::Expr::NumericLiteral(_)
            | ast::Expr::Param(_)
            | ast::Expr::Binary { .. }
            | ast::Expr::Between { .. }
            | ast::Expr::Like { .. }
            | ast::Expr::InList { .. }
            | ast::Expr::IsNull { .. }
            | ast::Expr::TruthTest { .. }
            | ast::Expr::Not { .. }
            | ast::Expr::Function(_)
            | ast::Expr::Exists(_)
            | ast::Expr::Case { .. } => None,
        }
    }
}

// Subquery/CTE names can retain raw delimiters. Reuse the existing identifier
// projection for their visible namespace; never expose their inner relation.
fn owns_alias(alias: &str, requested: &str) -> bool {
    ColumnIdentifierPath::parse(alias)
        .is_ok_and(|path| !path.is_qualified() && path.namespace_key() == requested)
}
