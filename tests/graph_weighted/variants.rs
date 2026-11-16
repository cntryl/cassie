//! The oracle suites of `graph_weighted_paths` again under every read path: the lazy
//! indexed store with endpoint indexes, and the exact full read without and with them. The
//! default run of those suites already covers the lazy store with no endpoint index (the
//! fallback), so together every combination answers identically (#1258, #1268).

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

fn suite() -> Vec<(&'static str, fn())> {
    vec![
        (
            "should_reject_a_hidden_negative_edge_even_when_the_cheapest_path_avoids_it",
            crate::oracle::should_reject_a_hidden_negative_edge_even_when_the_cheapest_path_avoids_it as fn(),
        ),
        (
            "should_reject_a_disconnected_invalid_edge",
            crate::oracle::should_reject_a_disconnected_invalid_edge as fn(),
        ),
        (
            "should_ignore_an_invalid_edge_that_the_label_filter_excludes",
            crate::oracle::should_ignore_an_invalid_edge_that_the_label_filter_excludes as fn(),
        ),
        (
            "should_reject_an_invalid_edge_of_a_selected_label",
            crate::oracle::should_reject_an_invalid_edge_of_a_selected_label as fn(),
        ),
        (
            "should_select_no_edge_for_an_empty_label_array",
            crate::oracle::should_select_no_edge_for_an_empty_label_array as fn(),
        ),
        (
            "should_return_the_zero_hop_path_before_the_closing_cycle_of_a_zero_cost_two_cycle",
            crate::oracle::should_return_the_zero_hop_path_before_the_closing_cycle_of_a_zero_cost_two_cycle as fn(),
        ),
        (
            "should_order_equal_cost_paths_by_the_contract_path_order",
            crate::oracle::should_order_equal_cost_paths_by_the_contract_path_order as fn(),
        ),
        (
            "should_order_parallel_edges_of_different_mappings_by_mapping_identity",
            crate::oracle::should_order_parallel_edges_of_different_mappings_by_mapping_identity as fn(),
        ),
        (
            "should_include_a_path_whose_cost_equals_max_cost",
            crate::oracle::should_include_a_path_whose_cost_equals_max_cost as fn(),
        ),
        (
            "should_exclude_paths_longer_than_max_hops",
            crate::oracle::should_exclude_paths_longer_than_max_hops as fn(),
        ),
        (
            "should_follow_edges_backwards_for_direction_in",
            crate::oracle::should_follow_edges_backwards_for_direction_in as fn(),
        ),
        (
            "should_follow_edges_both_ways_for_direction_both",
            crate::oracle::should_follow_edges_both_ways_for_direction_both as fn(),
        ),
        (
            "should_return_the_zero_hop_path_for_a_source_equal_to_its_target",
            crate::oracle::should_return_the_zero_hop_path_for_a_source_equal_to_its_target as fn(),
        ),
        (
            "should_return_a_self_loop_as_the_closing_path_of_its_vertex",
            crate::oracle::should_return_a_self_loop_as_the_closing_path_of_its_vertex as fn(),
        ),
        (
            "should_keep_fractional_weights_exact",
            crate::oracle::should_keep_fractional_weights_exact as fn(),
        ),
        (
            "should_agree_with_the_oracle_on_a_dense_graph_for_every_k",
            crate::oracle::should_agree_with_the_oracle_on_a_dense_graph_for_every_k as fn(),
        ),
        (
            "should_agree_with_the_oracle_when_an_in_range_cost_limit_cuts_the_answer",
            crate::oracle::should_agree_with_the_oracle_when_an_in_range_cost_limit_cuts_the_answer as fn(),
        ),
        (
            "should_fail_with_22003_when_a_cost_sum_is_unrepresentable",
            crate::oracle::should_fail_with_22003_when_a_cost_sum_is_unrepresentable as fn(),
        ),
        (
            "should_ignore_the_weight_of_an_edge_whose_endpoint_does_not_exist",
            crate::oracle::should_ignore_the_weight_of_an_edge_whose_endpoint_does_not_exist as fn(),
        ),
        (
            "should_agree_with_the_oracle_for_every_argument_combination",
            crate::matrix::should_agree_with_the_oracle_for_every_argument_combination as fn(),
        ),
        (
            "should_agree_with_the_oracle_when_an_invalid_weight_decides_the_answer",
            crate::matrix::should_agree_with_the_oracle_when_an_invalid_weight_decides_the_answer as fn(),
        ),
        (
            "should_order_mixed_mapping_ties_by_the_shared_reference_order",
            crate::mixed_mappings::should_order_mixed_mapping_ties_by_the_shared_reference_order as fn(),
        ),
    ]
}

#[test]
fn should_pass_the_weighted_suites_given_lazy_indexed_reads() {
    // Arrange
    let tests = suite();

    // Act
    // Assert
    run_all(LAZY_INDEXED, &tests);
}

#[test]
fn should_pass_the_weighted_suites_given_exact_plain_reads() {
    // Arrange
    let tests = suite();

    // Act
    // Assert
    run_all(EXACT_PLAIN, &tests);
}

#[test]
fn should_pass_the_weighted_suites_given_exact_indexed_reads() {
    // Arrange
    let tests = suite();

    // Act
    // Assert
    run_all(EXACT_INDEXED, &tests);
}
