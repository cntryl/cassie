//! Comparisons of a name-valued catalog column against a `regclass` value.
//!
//! Cassie reports `pg_attribute.attrelid` and `pg_index.indexrelid` as the
//! relation name and exposes the numeric identity as the `<column>_oid`
//! sibling. PostgreSQL types these columns as `oid`, so
//! `attrelid = 'orders'::regclass` is valid there. The comparison is bound to
//! the numeric sibling so both spellings work.
use crate::sql::ast::Expr;
use crate::types::{DataType, ObjectIdKind};

use super::super::coalesce_results::ResultTypes;

pub(super) fn bind_pair(left: &mut Expr, right: &mut Expr, types: &ResultTypes) {
    bind_side(left, right, types);
    bind_side(right, left, types);
}

fn bind_side(column: &mut Expr, other: &Expr, types: &ResultTypes) {
    let Expr::Column(name) = &*column else {
        return;
    };
    let sibling = Expr::Column(format!("{name}_oid"));
    if types.expression_type(other) != Some(DataType::ObjectId(ObjectIdKind::RegClass))
        || types.expression_type(column) != Some(DataType::Text)
    {
        return;
    }
    if types.expression_type(&sibling) == Some(DataType::Int) {
        *column = sibling;
    }
}
