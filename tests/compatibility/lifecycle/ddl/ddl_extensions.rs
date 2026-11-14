use super::*;

#[path = "atomic_drop.rs"]
mod atomic_drop;
#[path = "atomic_lifecycle.rs"]
mod issue_839_atomic_lifecycle;
#[path = "if_exists_notices.rs"]
mod issue_839_if_exists_notices;
#[path = "rebuild_swap.rs"]
mod rebuild_swap;
#[path = "recreate_relation.rs"]
mod recreate_relation;
#[path = "../sequences/sequence_identity.rs"]
mod sequence_identity_wave7;

#[path = "../extensions/extension_catalog_coverage.rs"]
mod extension_catalog_coverage;
#[path = "../extensions/catalog_descriptors.rs"]
mod extension_catalog_descriptors;
#[path = "../extensions/dependency_targets.rs"]
mod extension_profile_rejections;
#[path = "../indexes/index.rs"]
mod index_contracts;
#[path = "../extensions/pgcrypto_extension.rs"]
mod pgcrypto_extension;
#[path = "../tables/table_forms.rs"]
mod table_forms;
#[path = "../tables/table_storage_access.rs"]
mod table_storage_access;
#[path = "../tables/table_storage_like.rs"]
mod table_storage_like;
#[path = "conversion.rs"]
mod type_conversion;
#[path = "../extensions/vector_extension.rs"]
mod vector_extension;
#[path = "../extensions/vector_type_identity.rs"]
mod vector_type_identity;

include!("ddl_extensions/semantics.rs");

include!("ddl_extensions/transactions.rs");

include!("../sequences/ddl_extensions/metadata.rs");

include!("../sequences/ddl_extensions/semantics.rs");
