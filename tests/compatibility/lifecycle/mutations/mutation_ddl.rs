use super::*;

#[path = "../conflicts/conditional_upsert.rs"]
mod conditional_upsert_contract;
#[path = "../conflicts/conflict_opclass_inference.rs"]
mod conflict_opclass_inference;
#[path = "partial_index_predicate_inference.rs"]
mod partial_index_predicate_inference;

include!("../conflicts/mutation_ddl/parameters.rs");

include!("../conflicts/mutation_ddl/semantics.rs");

include!("../ctes/mutation_ddl/semantics.rs");

include!("../delete_using/mutation_ddl/semantics.rs");

include!("../indexes/mutation_ddl/semantics.rs");

include!("../materialized_views/mutation_ddl/semantics.rs");

include!("../merge/mutation_ddl/errors.rs");

include!("../merge/mutation_ddl/semantics.rs");

include!("mutation_ddl/nulls.rs");

include!("mutation_ddl/restart.rs");

include!("../namespaces/mutation_ddl/semantics.rs");

include!("../sequences/mutation_ddl/semantics.rs");

include!("../truncate/mutation_ddl/nulls.rs");

include!("../truncate/mutation_ddl/semantics.rs");

include!("../types/mutation_ddl/semantics.rs");

include!("../update_from/mutation_ddl/semantics.rs");
