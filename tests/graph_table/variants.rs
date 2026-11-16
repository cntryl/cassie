//! The oracle, semantics, transaction and composition suites again under every read path:
//! the lazy indexed store with endpoint indexes, the exact full read without and with
//! them. The default run of those suites already covers the lazy store with no endpoint
//! index (the fallback), so together every combination answers identically (#1253, #1254).

use crate::world::{Variant, VariantGuard};

fn run_all(variant: Variant, tests: &[(&str, fn())]) {
    let _guard = VariantGuard::set(variant);
    for (name, test) in tests {
        if let Err(panic) = std::panic::catch_unwind(*test) {
            eprintln!("variant failed in {name}");
            std::panic::resume_unwind(panic);
        }
    }
}

const LAZY_INDEXED: Variant = Variant {
    exact: false,
    endpoint_indexes: true,
};
const EXACT_PLAIN: Variant = Variant {
    exact: true,
    endpoint_indexes: false,
};
const EXACT_INDEXED: Variant = Variant {
    exact: true,
    endpoint_indexes: true,
};

fn matrix_tests() -> Vec<(&'static str, fn())> {
    vec![
        (
            "should_agree_with_the_oracle_for_every_mode_direction_bound_selector",
            crate::matrix::should_agree_with_the_oracle_for_every_mode_direction_bound_selector
                as fn(),
        ),
        (
            "should_agree_with_the_oracle_for_unbounded_quantifiers_under_finite_modes",
            crate::matrix::should_agree_with_the_oracle_for_unbounded_quantifiers_under_finite_modes
                as fn(),
        ),
    ]
}

fn semantics_tests() -> Vec<(&'static str, fn())> {
    vec![
        (
            "should_match_oracle_for_single_hop_edges",
            crate::semantics::should_match_oracle_for_single_hop_edges as fn(),
        ),
        (
            "should_return_every_vertex_for_a_vertex_only_pattern",
            crate::semantics::should_return_every_vertex_for_a_vertex_only_pattern as fn(),
        ),
        (
            "should_honour_inclusive_bounds_of_two_to_three_hops",
            crate::semantics::should_honour_inclusive_bounds_of_two_to_three_hops as fn(),
        ),
        (
            "should_bind_both_endpoints_of_a_zero_hop_path_to_one_vertex",
            crate::semantics::should_bind_both_endpoints_of_a_zero_hop_path_to_one_vertex as fn(),
        ),
        (
            "should_require_both_endpoint_predicates_on_a_zero_hop_vertex",
            crate::semantics::should_require_both_endpoint_predicates_on_a_zero_hop_vertex
                as fn(),
        ),
        (
            "should_apply_inline_predicates_before_selection_unlike_the_graph_where",
            crate::semantics::should_apply_inline_predicates_before_selection_unlike_the_graph_where
                as fn(),
        ),
        (
            "should_distinguish_simple_from_acyclic_on_a_two_cycle",
            crate::semantics::should_distinguish_simple_from_acyclic_on_a_two_cycle as fn(),
        ),
        (
            "should_enumerate_bounded_walks_like_the_oracle",
            crate::semantics::should_enumerate_bounded_walks_like_the_oracle as fn(),
        ),
        (
            "should_enumerate_unbounded_trails_like_the_oracle",
            crate::semantics::should_enumerate_unbounded_trails_like_the_oracle as fn(),
        ),
        (
            "should_traverse_an_undirected_self_loop_once",
            crate::semantics::should_traverse_an_undirected_self_loop_once as fn(),
        ),
        (
            "should_follow_edges_against_their_direction_like_the_oracle",
            crate::semantics::should_follow_edges_against_their_direction_like_the_oracle as fn(),
        ),
        (
            "should_follow_undirected_edges_in_both_orientations_like_the_oracle",
            crate::semantics::should_follow_undirected_edges_in_both_orientations_like_the_oracle
                as fn(),
        ),
        (
            "should_keep_parallel_edges_as_distinct_matches",
            crate::semantics::should_keep_parallel_edges_as_distinct_matches as fn(),
        ),
        (
            "should_choose_the_path_through_the_lower_edge_on_a_shortest_tie",
            crate::semantics::should_choose_the_path_through_the_lower_edge_on_a_shortest_tie
                as fn(),
        ),
        (
            "should_return_the_k_lowest_tied_paths_per_endpoint_pair",
            crate::semantics::should_return_the_k_lowest_tied_paths_per_endpoint_pair as fn(),
        ),
        (
            "should_apply_an_inline_edge_predicate_to_each_group_edge",
            crate::semantics::should_apply_an_inline_edge_predicate_to_each_group_edge as fn(),
        ),
        (
            "should_bind_a_repeated_variable_to_one_vertex",
            crate::semantics::should_bind_a_repeated_variable_to_one_vertex as fn(),
        ),
        (
            "should_reject_a_predicate_that_is_not_true",
            crate::semantics::should_reject_a_predicate_that_is_not_true as fn(),
        ),
        (
            "should_agree_with_the_oracle_across_key_shapes",
            crate::semantics::should_agree_with_the_oracle_across_key_shapes as fn(),
        ),
        (
            "should_agree_with_the_oracle_on_columnar_tables",
            crate::semantics::should_agree_with_the_oracle_on_columnar_tables as fn(),
        ),
        (
            "should_count_a_path_once_across_adjacent_quantified_steps",
            crate::semantics::should_count_a_path_once_across_adjacent_quantified_steps as fn(),
        ),
        (
            "should_evaluate_an_edge_predicate_that_reads_another_variable",
            crate::semantics::should_evaluate_an_edge_predicate_that_reads_another_variable
                as fn(),
        ),
        (
            "should_return_each_binding_of_a_named_vertex_between_quantified_steps",
            crate::semantics::should_return_each_binding_of_a_named_vertex_between_quantified_steps
                as fn(),
        ),
    ]
}

fn paths_tests() -> Vec<(&'static str, fn())> {
    vec![
        (
            "should_bind_a_projected_path_to_the_graph_path_type",
            crate::paths::should_bind_a_projected_path_to_the_graph_path_type as fn(),
        ),
        (
            "should_project_the_contract_path_example",
            crate::paths::should_project_the_contract_path_example as fn(),
        ),
        (
            "should_project_a_zero_hop_path_with_one_vertex_without_edges",
            crate::paths::should_project_a_zero_hop_path_with_one_vertex_without_edges as fn(),
        ),
        (
            "should_carry_the_durable_identities_of_the_graph_elements",
            crate::paths::should_carry_the_durable_identities_of_the_graph_elements as fn(),
        ),
        (
            "should_carry_composite_keys_in_key_order",
            crate::paths::should_carry_composite_keys_in_key_order as fn(),
        ),
        (
            "should_carry_uuid_keys_as_uuid_parts",
            crate::paths::should_carry_uuid_keys_as_uuid_parts as fn(),
        ),
        (
            "should_preserve_the_declared_width_of_integer_keys",
            crate::paths::should_preserve_the_declared_width_of_integer_keys as fn(),
        ),
        (
            "should_record_reverse_plus_undirected_directions_like_the_oracle_does",
            crate::paths::should_record_reverse_plus_undirected_directions_like_the_oracle_does
                as fn(),
        ),
        (
            "should_match_the_oracle_for_variable_length_projected_paths",
            crate::paths::should_match_the_oracle_for_variable_length_projected_paths as fn(),
        ),
        (
            "should_choose_the_tied_shortest_path_by_the_contract_total_order",
            crate::paths::should_choose_the_tied_shortest_path_by_the_contract_total_order
                as fn(),
        ),
        (
            "should_choose_the_k_lowest_tied_paths_by_the_contract_total_order",
            crate::paths::should_choose_the_k_lowest_tied_paths_by_the_contract_total_order
                as fn(),
        ),
        (
            "should_export_a_projected_path_to_jsonb_without_rounding_bigint_keys",
            crate::paths::should_export_a_projected_path_to_jsonb_without_rounding_bigint_keys
                as fn(),
        ),
        (
            "should_list_the_steps_of_a_projected_path",
            crate::paths::should_list_the_steps_of_a_projected_path as fn(),
        ),
        (
            "should_read_back_a_projected_path_stored_in_a_graph_path_column",
            crate::paths::should_read_back_a_projected_path_stored_in_a_graph_path_column as fn(),
        ),
        (
            "should_accept_a_projected_path_back_as_a_parameter",
            crate::paths::should_accept_a_projected_path_back_as_a_parameter as fn(),
        ),
        (
            "should_resolve_a_reference_constructor_like_the_pattern_source",
            crate::paths::should_resolve_a_reference_constructor_like_the_pattern_source as fn(),
        ),
    ]
}

fn exact_reads_tests() -> Vec<(&'static str, fn())> {
    vec![
        (
            "should_exclude_an_edge_whose_endpoint_vertex_is_missing",
            crate::exact_reads::should_exclude_an_edge_whose_endpoint_vertex_is_missing as fn(),
        ),
        (
            "should_show_a_dangling_edge_once_its_endpoint_is_inserted",
            crate::exact_reads::should_show_a_dangling_edge_once_its_endpoint_is_inserted as fn(),
        ),
        (
            "should_select_several_mappings_that_share_a_label",
            crate::exact_reads::should_select_several_mappings_that_share_a_label as fn(),
        ),
        (
            "should_not_cross_endpoint_mappings_of_an_edge",
            crate::exact_reads::should_not_cross_endpoint_mappings_of_an_edge as fn(),
        ),
        (
            "should_count_a_mapping_carrying_both_listed_labels_once",
            crate::exact_reads::should_count_a_mapping_carrying_both_listed_labels_once as fn(),
        ),
        (
            "should_leave_out_an_edge_with_a_null_endpoint_component",
            crate::exact_reads::should_leave_out_an_edge_with_a_null_endpoint_component as fn(),
        ),
    ]
}

fn transactions_tests() -> Vec<(&'static str, fn())> {
    vec![
        (
            "should_see_own_uncommitted_inserts",
            crate::transactions::should_see_own_uncommitted_inserts as fn(),
        ),
        (
            "should_hide_uncommitted_writes_from_another_session_until_commit",
            crate::transactions::should_hide_uncommitted_writes_from_another_session_until_commit
                as fn(),
        ),
        (
            "should_see_an_own_update_that_changes_an_inline_predicate",
            crate::transactions::should_see_an_own_update_that_changes_an_inline_predicate
                as fn(),
        ),
        (
            "should_see_own_deletes",
            crate::transactions::should_see_own_deletes as fn(),
        ),
        (
            "should_follow_repeated_mutation_of_one_element",
            crate::transactions::should_follow_repeated_mutation_of_one_element as fn(),
        ),
        (
            "should_roll_back_to_a_savepoint",
            crate::transactions::should_roll_back_to_a_savepoint as fn(),
        ),
        (
            "should_keep_a_repeatable_read_snapshot_across_a_concurrent_commit",
            crate::transactions::should_keep_a_repeatable_read_snapshot_across_a_concurrent_commit
                as fn(),
        ),
    ]
}

fn composition_tests() -> Vec<(&'static str, fn())> {
    vec![
        (
            "should_filter_a_graph_table_with_an_outer_where",
            crate::composition::should_filter_a_graph_table_with_an_outer_where as fn(),
        ),
        (
            "should_join_a_graph_table_with_an_ordinary_table",
            crate::composition::should_join_a_graph_table_with_an_ordinary_table as fn(),
        ),
        (
            "should_correlate_a_lateral_graph_table_per_outer_row",
            crate::composition::should_correlate_a_lateral_graph_table_per_outer_row as fn(),
        ),
        (
            "should_aggregate_over_a_graph_table",
            crate::composition::should_aggregate_over_a_graph_table as fn(),
        ),
        (
            "should_use_a_graph_table_inside_exists",
            crate::composition::should_use_a_graph_table_inside_exists as fn(),
        ),
        (
            "should_read_a_graph_table_through_a_derived_table",
            crate::composition::should_read_a_graph_table_through_a_derived_table as fn(),
        ),
        (
            "should_read_a_view_over_a_graph_table",
            crate::composition::should_read_a_view_over_a_graph_table as fn(),
        ),
        (
            "should_bind_parameters_in_a_prepared_graph_table",
            crate::composition::should_bind_parameters_in_a_prepared_graph_table as fn(),
        ),
        (
            "should_reevaluate_a_lateral_parameter_for_each_outer_row",
            crate::composition::should_reevaluate_a_lateral_parameter_for_each_outer_row as fn(),
        ),
    ]
}

#[test]
fn should_pass_the_matrix_suite_given_lazy_indexed_reads() {
    // Arrange
    let tests = matrix_tests();

    // Act
    // Assert
    run_all(LAZY_INDEXED, &tests);
}

#[test]
fn should_pass_the_matrix_suite_given_exact_plain_reads() {
    // Arrange
    let tests = matrix_tests();

    // Act
    // Assert
    run_all(EXACT_PLAIN, &tests);
}

#[test]
fn should_pass_the_matrix_suite_given_exact_indexed_reads() {
    // Arrange
    let tests = matrix_tests();

    // Act
    // Assert
    run_all(EXACT_INDEXED, &tests);
}

#[test]
fn should_pass_the_semantics_suite_given_lazy_indexed_reads() {
    // Arrange
    let tests = semantics_tests();

    // Act
    // Assert
    run_all(LAZY_INDEXED, &tests);
}

#[test]
fn should_pass_the_semantics_suite_given_exact_plain_reads() {
    // Arrange
    let tests = semantics_tests();

    // Act
    // Assert
    run_all(EXACT_PLAIN, &tests);
}

#[test]
fn should_pass_the_semantics_suite_given_exact_indexed_reads() {
    // Arrange
    let tests = semantics_tests();

    // Act
    // Assert
    run_all(EXACT_INDEXED, &tests);
}

#[test]
fn should_pass_the_paths_suite_given_lazy_indexed_reads() {
    // Arrange
    let tests = paths_tests();

    // Act
    // Assert
    run_all(LAZY_INDEXED, &tests);
}

#[test]
fn should_pass_the_paths_suite_given_exact_plain_reads() {
    // Arrange
    let tests = paths_tests();

    // Act
    // Assert
    run_all(EXACT_PLAIN, &tests);
}

#[test]
fn should_pass_the_paths_suite_given_exact_indexed_reads() {
    // Arrange
    let tests = paths_tests();

    // Act
    // Assert
    run_all(EXACT_INDEXED, &tests);
}

#[test]
fn should_pass_the_exact_reads_suite_given_lazy_indexed_reads() {
    // Arrange
    let tests = exact_reads_tests();

    // Act
    // Assert
    run_all(LAZY_INDEXED, &tests);
}

#[test]
fn should_pass_the_exact_reads_suite_given_exact_plain_reads() {
    // Arrange
    let tests = exact_reads_tests();

    // Act
    // Assert
    run_all(EXACT_PLAIN, &tests);
}

#[test]
fn should_pass_the_exact_reads_suite_given_exact_indexed_reads() {
    // Arrange
    let tests = exact_reads_tests();

    // Act
    // Assert
    run_all(EXACT_INDEXED, &tests);
}

#[test]
fn should_pass_the_transactions_suite_given_lazy_indexed_reads() {
    // Arrange
    let tests = transactions_tests();

    // Act
    // Assert
    run_all(LAZY_INDEXED, &tests);
}

#[test]
fn should_pass_the_transactions_suite_given_exact_plain_reads() {
    // Arrange
    let tests = transactions_tests();

    // Act
    // Assert
    run_all(EXACT_PLAIN, &tests);
}

#[test]
fn should_pass_the_transactions_suite_given_exact_indexed_reads() {
    // Arrange
    let tests = transactions_tests();

    // Act
    // Assert
    run_all(EXACT_INDEXED, &tests);
}

#[test]
fn should_pass_the_composition_suite_given_lazy_indexed_reads() {
    // Arrange
    let tests = composition_tests();

    // Act
    // Assert
    run_all(LAZY_INDEXED, &tests);
}

#[test]
fn should_pass_the_composition_suite_given_exact_plain_reads() {
    // Arrange
    let tests = composition_tests();

    // Act
    // Assert
    run_all(EXACT_PLAIN, &tests);
}

#[test]
fn should_pass_the_composition_suite_given_exact_indexed_reads() {
    // Arrange
    let tests = composition_tests();

    // Act
    // Assert
    run_all(EXACT_INDEXED, &tests);
}
