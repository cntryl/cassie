// Integration suite: column-authoritative tables expose the same SQL surface
// as row tables (issues #1109, #1110, #1111; docs/columnar-table-contract.md).

#[path = "columnar_surface/pair.rs"]
mod pair;

#[path = "columnar_surface/query_equivalence.rs"]
mod query_equivalence;

#[path = "columnar_surface/constraint_equivalence.rs"]
mod constraint_equivalence;

#[path = "columnar_surface/index_equivalence.rs"]
mod index_equivalence;

#[path = "columnar_surface/grouped_aggregate.rs"]
mod grouped_aggregate;

#[path = "columnar_surface/pruning_equivalence.rs"]
mod pruning_equivalence;

#[path = "columnar_surface/vector_execution.rs"]
mod vector_execution;

#[path = "columnar_surface/vector_pruning.rs"]
mod vector_pruning;

#[path = "columnar_surface/nul_row_id.rs"]
mod nul_row_id;

#[path = "columnar_surface/vector_parallel.rs"]
mod vector_parallel;
