//! Opt-in embedding workload machinery for #1365 (workstation only, never CI).
//!
//! The pure modules (manifest, vector files, ground truth, statistics) are shared
//! with the network-free tests in `tests/vector_embeddings`. The Cassie runner and
//! the result writer are only compiled into the `tier5_embedding_workloads` bench.

#[path = "embedding_workloads/ground_truth.rs"]
pub mod ground_truth;
#[path = "embedding_workloads/manifest.rs"]
pub mod manifest;
#[path = "embedding_workloads/statistics.rs"]
pub mod statistics;
#[path = "embedding_workloads/synthetic.rs"]
pub mod synthetic;
#[path = "embedding_workloads/vector_file.rs"]
pub mod vector_file;
