//! Static argument identity for selected uncompressed PostgreSQL datum sizes.
use crate::app::CassieError;
use crate::sql::ast::{CastIdentity, Expr};
use crate::types::DataType;

#[derive(Clone, Copy)]
pub(crate) enum DatumSizeKind {
    Fixed(usize),
    Unknown,
    Text,
    Bytea,
}

pub(crate) fn is_column_size(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_column_size")
        || name.eq_ignore_ascii_case("pg_catalog.pg_column_size")
}

pub(crate) fn datum_size_kind(expression: &Expr) -> Result<DatumSizeKind, CassieError> {
    // These casts lack the selected datum layout or can expand before the
    // observer. Reject them before constant planning or argument evaluation.
    if expression.any_descendant_or_self(&mut |expr| {
        matches!(
            expr,
            Expr::Cast {
                identity: CastIdentity::Numeric,
                ..
            } | Expr::Cast {
                data_type: DataType::Char { .. },
                ..
            }
        )
    }) {
        return Err(pending());
    }
    match expression {
        Expr::Null => Ok(DatumSizeKind::Fixed(0)),
        Expr::BoolLiteral(_) => fixed(&DataType::Boolean),
        Expr::IntegerLiteral(value) => fixed(&if i32::try_from(*value).is_ok() {
            DataType::Int
        } else {
            DataType::BigInt
        }),
        Expr::StringLiteral(_) => Ok(DatumSizeKind::Unknown),
        Expr::Cast { data_type, .. } if data_type.typlen() > 0 => fixed(data_type),
        Expr::Cast {
            data_type, expr, ..
        } if materialized(expr) => match data_type {
            DataType::Text | DataType::Varchar { .. } => Ok(DatumSizeKind::Text),
            DataType::Bytea => Ok(DatumSizeKind::Bytea),
            _ => Err(pending()),
        },
        _ => Err(pending()),
    }
}
fn fixed(data_type: &DataType) -> Result<DatumSizeKind, CassieError> {
    usize::try_from(data_type.typlen())
        .map(DatumSizeKind::Fixed)
        .map_err(|_| pending())
}
fn pending() -> CassieError {
    CassieError::Unsupported("pg_column_size argument identity/layout remains pending under #793/#817; selected primitive casts and materialized text/bytea inputs are required".into())
}

fn materialized(expression: &Expr) -> bool {
    match expression {
        Expr::StringLiteral(_) | Expr::Param(_) | Expr::Null => true,
        Expr::Cast {
            expr,
            data_type: DataType::Text | DataType::Varchar { .. } | DataType::Bytea,
            ..
        } => materialized(expr),
        _ => false,
    }
}
