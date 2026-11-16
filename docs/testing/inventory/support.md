# support test placements

Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.

`support` is a placement group, not a Cargo target. Helper tests compile inside the Cargo suites listed in each inventory record's `instances` field.

| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_differ_between_inline_versus_post_selection_predicates` | [tests/support/graph_oracle/tests.rs:90](../../../tests/support/graph_oracle/tests.rs#L90) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_select_two_hop_path_with_bounds_two_to_three` | [tests/support/graph_oracle/tests.rs:126](../../../tests/support/graph_oracle/tests.rs#L126) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_bind_one_vertex_for_zero_hop_paths` | [tests/support/graph_oracle/tests.rs:143](../../../tests/support/graph_oracle/tests.rs#L143) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_distinguish_simple_from_acyclic_on_two_cycle` | [tests/support/graph_oracle/tests.rs:169](../../../tests/support/graph_oracle/tests.rs#L169) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_treat_undirected_self_loop_as_one_step` | [tests/support/graph_oracle/tests.rs:189](../../../tests/support/graph_oracle/tests.rs#L189) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_record_reverse_direction_for_undirected_non_loop_edge` | [tests/support/graph_oracle/tests.rs:204](../../../tests/support/graph_oracle/tests.rs#L204) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_keep_parallel_edges_as_distinct_paths` | [tests/support/graph_oracle/tests.rs:222](../../../tests/support/graph_oracle/tests.rs#L222) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_match_label_alternatives_once` | [tests/support/graph_oracle/tests.rs:236](../../../tests/support/graph_oracle/tests.rs#L236) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_break_equal_length_tie_by_path_order` | [tests/support/graph_oracle/tests.rs:253](../../../tests/support/graph_oracle/tests.rs#L253) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_select_shortest_k_per_endpoint_pair` | [tests/support/graph_oracle/tests.rs:277](../../../tests/support/graph_oracle/tests.rs#L277) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_exclude_edges_with_absent_endpoint` | [tests/support/graph_oracle/tests.rs:302](../../../tests/support/graph_oracle/tests.rs#L302) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_order_zero_hop_path_before_closing_cycle` | [tests/support/graph_oracle/tests.rs:314](../../../tests/support/graph_oracle/tests.rs#L314) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_reject_unbounded_walk_but_allow_unbounded_trail` | [tests/support/graph_oracle/tests.rs:330](../../../tests/support/graph_oracle/tests.rs#L330) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_reject_inverted_bounds_or_zero_k` | [tests/support/graph_oracle/tests.rs:346](../../../tests/support/graph_oracle/tests.rs#L346) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_fail_whole_call_for_hidden_negative_weight` | [tests/support/graph_oracle/tests.rs:361](../../../tests/support/graph_oracle/tests.rs#L361) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_reject_null_missing_or_non_finite_weights` | [tests/support/graph_oracle/tests.rs:379](../../../tests/support/graph_oracle/tests.rs#L379) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_ignore_weights_of_dangling_edges` | [tests/support/graph_oracle/tests.rs:407](../../../tests/support/graph_oracle/tests.rs#L407) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_return_zero_hop_then_closing_cycle_for_zero_cost_two_cycle` | [tests/support/graph_oracle/tests.rs:421](../../../tests/support/graph_oracle/tests.rs#L421) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `should_rank_cheapest_paths_by_tie_order_within_cost_limit` | [tests/support/graph_oracle/tests.rs:447](../../../tests/support/graph_oracle/tests.rs#L447) | `tests/support/graph_oracle/tests.rs` | registered | no | 1 | recommended |
| `seed_array_tests::should_require_seed_array_shape_with_sql_null_slots` | [tests/support/relational_qualification.rs:208](../../../tests/support/relational_qualification.rs#L208) | `tests/support/relational_qualification.rs` | registered | no | 1 | recommended |
| `should_validate_script_step_error_counts` | [tests/support/repeatable_read.rs:256](../../../tests/support/repeatable_read.rs#L256) | `tests/support/repeatable_read.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_drop_sql_fixture_owners_before_removing_directory` | [tests/support/sql_fixture.rs:191](../../../tests/support/sql_fixture.rs#L191) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_report_directory_cleanup_failure_on_normal_drop` | [tests/support/sql_fixture.rs:234](../../../tests/support/sql_fixture.rs#L234) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_report_cleanup_error_without_replacing_an_existing_panic` | [tests/support/sql_fixture.rs:254](../../../tests/support/sql_fixture.rs#L254) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_report_cleanup_error_during_unwind_without_a_subscriber` | [tests/support/sql_fixture.rs:282](../../../tests/support/sql_fixture.rs#L282) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `tests::should_keep_a_directory_a_running_test_still_owns` | [tests/support/temp_dirs.rs:95](../../../tests/support/temp_dirs.rs#L95) | `tests/support/temp_dirs.rs` | registered | no | 1 | recommended |
| `tests::should_treat_a_directory_older_than_the_window_as_stale` | [tests/support/temp_dirs.rs:114](../../../tests/support/temp_dirs.rs#L114) | `tests/support/temp_dirs.rs` | registered | no | 1 | recommended |
| `tests::should_sweep_only_once_per_process` | [tests/support/temp_dirs.rs:136](../../../tests/support/temp_dirs.rs#L136) | `tests/support/temp_dirs.rs` | registered | no | 1 | recommended |
