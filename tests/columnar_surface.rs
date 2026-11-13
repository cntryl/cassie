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
