//! Type declaration changes published atomically within one database.

use cassie_core::catalog::CompositeTypeMeta;
use cassie_core::types::DataType;

use crate::routine_declarations::ViewMeta;

/// One table column whose embedded descriptor must be re-published.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnRetype {
    pub table: String,
    pub column: String,
    pub data_type: DataType,
}

/// Everything one `ALTER TYPE` (or row-type refresh) writes into one database
/// family, published as a single Midge transaction.
#[derive(Debug, Clone, Default)]
pub struct CompositeGroup {
    pub database: String,
    /// The altered type itself; present only in the target's database.
    pub target: Option<CompositeTypeMeta>,
    /// Old name of a renamed target, deleted in the same transaction.
    pub deleted_type: Option<String>,
    pub types: Vec<CompositeTypeMeta>,
    pub columns: Vec<ColumnRetype>,
    pub views: Vec<ViewMeta>,
}

/// Every object that embeds a changed enum descriptor.
#[derive(Debug, Clone, Default)]
pub struct EnumPropagation {
    pub columns: Vec<ColumnRetype>,
    pub views: Vec<ViewMeta>,
}
