use super::*;

#[path = "atomic_drop.rs"]
mod atomic_drop;
#[path = "../partitions/default_range_partition_routing.rs"]
mod default_range_partition_routing;
#[path = "../extensions/extension_catalog_coverage.rs"]
mod extension_catalog_coverage;
#[path = "../extensions/extension_schema_relocation.rs"]
mod extension_schema_relocation;
#[path = "atomic_lifecycle.rs"]
mod issue_839_atomic_lifecycle;
#[path = "../partitions/lifecycle.rs"]
mod issue_840_partition_lifecycle;
#[path = "../namespaces/database_temp_lifecycle.rs"]
mod postgresql18_database_temp_lifecycle;
#[path = "../sequences/sequence_identity.rs"]
mod sequence_identity_wave7;

#[path = "../indexes/index.rs"]
mod index_contracts;
#[path = "maintenance_statistics_refresh.rs"]
mod maintenance_statistics_refresh;
#[path = "../partitions/multilevel_partition_routing.rs"]
mod multilevel_partition_routing;
#[path = "../partitions/partition_information_functions.rs"]
mod partition_information_functions;
#[path = "../extensions/pgcrypto_extension.rs"]
mod pgcrypto_extension;
#[path = "../tables/table_forms.rs"]
mod table_forms;
#[path = "../tables/table_storage_access.rs"]
mod table_storage_access;
#[path = "conversion.rs"]
mod type_conversion;

include!("ddl_extensions/semantics.rs");

include!("ddl_extensions/transactions.rs");

include!("../partitions/ddl_extensions/semantics.rs");

include!("../partitions/ddl_extensions/transactions.rs");

include!("../sequences/ddl_extensions/metadata.rs");

include!("../sequences/ddl_extensions/semantics.rs");
