//! Native parsed index and sequence name boundaries, observed as TEXT.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_catalog_name_0_given_parsed_byte_boundary_when_observed_as_text() {
    // Arrange
    let sql = "SELECT indexname::TEXT AS output FROM pg_indexes WHERE indexname='i9_index_0_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'";
    // Act
    let batches = planning::pruned_with_setup("identifier-catalog-0", sql, &["CREATE TABLE i9_catalog_0 (v INTEGER)","CREATE INDEX \"i9_index_0_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\" ON i9_catalog_0(v)"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 105, 110, 100, 101, 120, 95, 48, 95, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97,
            ]],
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 105, 110, 100, 101, 120, 95, 48, 95, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97,
            ]],
        ],
    );
}

#[test]
fn should_preserve_catalog_name_1_given_parsed_byte_boundary_when_observed_as_text() {
    // Arrange
    let sql = "SELECT indexname::TEXT AS output FROM pg_indexes WHERE indexname='i9_index_1_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'";
    // Act
    let batches = planning::pruned_with_setup("identifier-catalog-1", sql, &["CREATE TABLE i9_catalog_1 (v INTEGER)","CREATE INDEX I9_INDEX_1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA ON i9_catalog_1(v)"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 105, 110, 100, 101, 120, 95, 49, 95, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97,
            ]],
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 105, 110, 100, 101, 120, 95, 49, 95, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97,
            ]],
        ],
    );
}

#[test]
fn should_preserve_catalog_name_2_given_parsed_byte_boundary_when_observed_as_text() {
    // Arrange
    let sql = "SELECT sequencename::TEXT AS output FROM pg_sequences WHERE sequencename='i9_sequence_2_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'";
    // Act
    let batches = planning::pruned_with_setup("identifier-catalog-2", sql, &["CREATE SEQUENCE \"i9_sequence_2_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaéxyz\""]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 115, 101, 113, 117, 101, 110, 99, 101, 95, 50, 95,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97,
            ]],
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 115, 101, 113, 117, 101, 110, 99, 101, 95, 50, 95,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97, 97,
                97, 97, 97, 97, 97, 97, 97,
            ]],
        ],
    );
}

#[test]
fn should_bound_each_qualified_sequence_component_given_sql_name_when_observed_as_text() {
    // Arrange
    let sql = "SELECT sequencename::TEXT AS output FROM pg_sequences WHERE sequencename='i9_qualified_sequence_ccccccccccccccccccccccccccccccccccccccccc'";
    // Act
    let batches = planning::pruned_with_setup("identifier-qualified-sequence", sql, &["CREATE SEQUENCE public.\"i9_qualified_sequence_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\""]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 113, 117, 97, 108, 105, 102, 105, 101, 100, 95,
                115, 101, 113, 117, 101, 110, 99, 101, 95, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
                99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
                99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
            ]],
            &[&[
                0, 1, 0, 0, 0, 63, 105, 57, 95, 113, 117, 97, 108, 105, 102, 105, 101, 100, 95,
                115, 101, 113, 117, 101, 110, 99, 101, 95, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
                99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
                99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
            ]],
        ],
    );
}
