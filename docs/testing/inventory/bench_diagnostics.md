# bench_diagnostics test placements

Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.

| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_admit_only_the_declared_graph_topology_points` | [tests/bench_diagnostics/graph_topology.rs:7](../../../tests/bench_diagnostics/graph_topology.rs#L7) | `tests/bench_diagnostics/semantics/graph_topology.rs` | registered | no | 1 | recommended |
| `should_reject_changes_to_the_topology_owner_workload_scale_or_class` | [tests/bench_diagnostics/graph_topology.rs:45](../../../tests/bench_diagnostics/graph_topology.rs#L45) | `tests/bench_diagnostics/semantics/graph_topology.rs` | registered | no | 1 | recommended |
| `should_resolve_a_memory_profile_for_every_registered_topology_scenario` | [tests/bench_diagnostics/graph_topology.rs:87](../../../tests/bench_diagnostics/graph_topology.rs#L87) | `tests/bench_diagnostics/semantics/graph_topology.rs` | registered | no | 1 | recommended |
| `should_reject_invalid_topology_catalog_contracts_without_relabeling_other_scales` | [tests/bench_diagnostics/graph_topology.rs:110](../../../tests/bench_diagnostics/graph_topology.rs#L110) | `tests/bench_diagnostics/semantics/graph_topology.rs` | registered | no | 1 | recommended |
| `should_return_column_aggregate_values_when_types_match` | [tests/bench_diagnostics/relations/semantics.rs:8](../../../tests/bench_diagnostics/relations/semantics.rs#L8) | `tests/bench_diagnostics/relations/semantics.rs` | registered | no | 1 | selected |
| `should_panic_with_redacted_types_for_unexpected_column_aggregate_values` | [tests/bench_diagnostics/relations/semantics.rs:30](../../../tests/bench_diagnostics/relations/semantics.rs#L30) | `tests/bench_diagnostics/relations/semantics.rs` | registered | no | 1 | selected |
| `should_default_to_strict_replay_mode` | [tests/bench_diagnostics/replay_mode.rs:10](../../../tests/bench_diagnostics/replay_mode.rs#L10) | `tests/bench_diagnostics/semantics/replay_mode.rs` | registered | no | 1 | recommended |
| `should_select_each_explicit_replay_mode` | [tests/bench_diagnostics/replay_mode.rs:22](../../../tests/bench_diagnostics/replay_mode.rs#L22) | `tests/bench_diagnostics/semantics/replay_mode.rs` | registered | no | 1 | recommended |
| `should_reject_unknown_replay_mode` | [tests/bench_diagnostics/replay_mode.rs:34](../../../tests/bench_diagnostics/replay_mode.rs#L34) | `tests/bench_diagnostics/semantics/replay_mode.rs` | registered | no | 1 | recommended |
| `should_reject_non_unicode_replay_mode` | [tests/bench_diagnostics/replay_mode.rs:47](../../../tests/bench_diagnostics/replay_mode.rs#L47) | `tests/bench_diagnostics/semantics/replay_mode.rs` | registered | no | 1 | recommended |
| `should_report_the_selected_replay_api_policy` | [tests/bench_diagnostics/replay_mode.rs:60](../../../tests/bench_diagnostics/replay_mode.rs#L60) | `tests/bench_diagnostics/semantics/replay_mode.rs` | registered | no | 1 | recommended |
| `should_select_sql_projector_replay_modes` | [tests/bench_diagnostics/replay_mode.rs:86](../../../tests/bench_diagnostics/replay_mode.rs#L86) | `tests/bench_diagnostics/semantics/replay_mode.rs` | registered | no | 1 | recommended |
| `should_select_indexed_shape_unless_unindexed_is_requested` | [tests/bench_diagnostics/replay_mode.rs:108](../../../tests/bench_diagnostics/replay_mode.rs#L108) | `tests/bench_diagnostics/semantics/replay_mode.rs` | registered | no | 1 | recommended |
