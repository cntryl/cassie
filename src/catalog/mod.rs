pub mod collections;
pub mod indexes;
pub mod constraints;
pub mod metadata;
pub mod schemas;

pub use collections::{CollectionMeta, NamespaceMeta};
pub use indexes::{IndexKind, IndexMeta};
pub use constraints::{ConstraintCheck, ConstraintOperator, FieldConstraint};
pub use metadata::Catalog;
pub use schemas::{CollectionSchema, FieldMeta};
