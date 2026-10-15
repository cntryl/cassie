# bench_operations test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `tests::should_reject_non_finite_ann_recall_evidence` | [tests/bench_operations/ann_recall_validation.rs:99](../../../tests/bench_operations/ann_recall_validation.rs#L99) | `tests/bench_operations/artifacts/tests.rs` | registered | no | 1 | recommended |
| `tests::should_reject_non_finite_ann_recall_floor` | [tests/bench_operations/ann_recall_validation.rs:112](../../../tests/bench_operations/ann_recall_validation.rs#L112) | `tests/bench_operations/artifacts/tests.rs` | registered | no | 1 | recommended |
| `tests::should_reject_ann_recall_above_one` | [tests/bench_operations/ann_recall_validation.rs:125](../../../tests/bench_operations/ann_recall_validation.rs#L125) | `tests/bench_operations/artifacts/tests.rs` | registered | no | 1 | recommended |
| `tests::should_accept_finite_ann_recall_endpoints` | [tests/bench_operations/ann_recall_validation.rs:138](../../../tests/bench_operations/ann_recall_validation.rs#L138) | `tests/bench_operations/artifacts/tests.rs` | registered | no | 1 | recommended |
| `tests::should_reject_nan_recall_in_complete_registered_ann_artifact` | [tests/bench_operations/ann_recall_validation.rs:237](../../../tests/bench_operations/ann_recall_validation.rs#L237) | `tests/bench_operations/artifacts/tests.rs` | registered | no | 1 | recommended |
| `should_identify_arm_cpuinfo_without_model_name` | [tests/bench_operations/host_identity.rs:53](../../../tests/bench_operations/host_identity.rs#L53) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_render_heterogeneous_arm_cpu_parts_in_stable_order` | [tests/bench_operations/host_identity.rs:68](../../../tests/bench_operations/host_identity.rs#L68) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_reject_missing_malformed_or_incomplete_arm_cpu_identity` | [tests/bench_operations/host_identity.rs:83](../../../tests/bench_operations/host_identity.rs#L83) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_reject_duplicate_processor_ids_in_arm_cpuinfo` | [tests/bench_operations/host_identity.rs:104](../../../tests/bench_operations/host_identity.rs#L104) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_reject_arm_cpuinfo_larger_than_the_collection_bound` | [tests/bench_operations/host_identity.rs:116](../../../tests/bench_operations/host_identity.rs#L116) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_bound_cpuinfo_reads_before_parsing` | [tests/bench_operations/host_identity.rs:128](../../../tests/bench_operations/host_identity.rs#L128) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_apply_cpu_identity_consistently` | [tests/bench_operations/host_identity.rs:145](../../../tests/bench_operations/host_identity.rs#L145) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_keep_unknown_cpu_identity_when_host_data_is_unavailable` | [tests/bench_operations/host_identity.rs:164](../../../tests/bench_operations/host_identity.rs#L164) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_keep_case_insensitive_unknown_fallback_consistent` | [tests/bench_operations/host_identity.rs:181](../../../tests/bench_operations/host_identity.rs#L181) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_reject_sample_identity_that_disagrees_with_the_run` | [tests/bench_operations/host_identity.rs:199](../../../tests/bench_operations/host_identity.rs#L199) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
| `should_publish_identity_consistently_in_saved_artifacts` | [tests/bench_operations/host_identity.rs:221](../../../tests/bench_operations/host_identity.rs#L221) | `tests/bench_operations/artifacts/host_identity.rs` | registered | no | 1 | recommended |
