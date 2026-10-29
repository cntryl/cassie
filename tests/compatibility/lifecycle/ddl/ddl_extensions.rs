use super::*;

#[path = "atomic_drop.rs"]
mod atomic_drop;
#[path = "atomic_lifecycle.rs"]
mod issue_839_atomic_lifecycle;
#[path = "if_exists_notices.rs"]
mod issue_839_if_exists_notices;
#[path = "recreate_relation.rs"]
mod recreate_relation;
#[path = "../sequences/sequence_identity.rs"]
mod sequence_identity_wave7;

#[path = "../indexes/index.rs"]
mod index_contracts;
#[path = "../extensions/dependency_targets.rs"]
mod pgcrypto_installation_boundary;
#[path = "../tables/table_forms.rs"]
mod table_forms;
#[path = "../tables/table_storage_access.rs"]
mod table_storage_access;
#[path = "conversion.rs"]
mod type_conversion;

include!("ddl_extensions/semantics.rs");

include!("ddl_extensions/transactions.rs");

include!("../sequences/ddl_extensions/metadata.rs");

include!("../sequences/ddl_extensions/semantics.rs");
