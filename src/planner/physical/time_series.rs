use super::{Expr, IndexMeta, IndexKind, BTreeSet, BinaryOp};

pub(super) fn selected_time_series_index(
    collection: &str,
    filter: &Expr,
    indexes: &[IndexMeta],
) -> Option<String> {
    let range_fields = range_filter_fields(filter);
    if range_fields.is_empty() {
        return None;
    }
    indexes
        .iter()
        .filter(|index| index.collection == collection && index.kind == IndexKind::TimeSeries)
        .find(|index| {
            index
                .normalized_fields()
                .first()
                .is_some_and(|field| range_fields.contains(&field.to_ascii_lowercase()))
        })
        .map(|index| index.name.clone())
}

fn range_filter_fields(expr: &Expr) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    collect_range_filter_fields(expr, &mut fields);
    fields
}

fn collect_range_filter_fields(expr: &Expr, fields: &mut BTreeSet<String>) {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_range_filter_fields(left, fields);
            collect_range_filter_fields(right, fields);
        }
        Expr::Binary {
            left,
            op: BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte,
            right,
        } => {
            if let Expr::Column(name) = left.as_ref() {
                fields.insert(name.to_ascii_lowercase());
            }
            if let Expr::Column(name) = right.as_ref() {
                fields.insert(name.to_ascii_lowercase());
            }
        }
        Expr::Between { expr, .. } => {
            if let Expr::Column(name) = expr.as_ref() {
                fields.insert(name.to_ascii_lowercase());
            }
        }
        _ => {}
    }
}
