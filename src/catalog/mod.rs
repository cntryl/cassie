pub mod collections;
pub mod constraints;
pub mod indexes;
pub mod metadata;
pub mod programs;
pub mod roles;
pub mod schemas;
pub mod virtual_views;

pub use collections::{CollectionMeta, NamespaceMeta};
pub use constraints::{ConstraintCheck, ConstraintOperator, FieldConstraint};
pub use indexes::{IndexKind, IndexMeta};
pub use metadata::Catalog;
pub use programs::{FunctionArgMeta, FunctionMeta, ProcedureMeta, ViewMeta, Volatility};
pub use roles::{normalize_role_name, RoleMeta};
pub use schemas::{CollectionSchema, FieldMeta};
