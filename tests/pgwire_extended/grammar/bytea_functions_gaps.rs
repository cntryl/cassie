//! Focused native codec edge witnesses from implementation review.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_codec_edge_0_given_unicode_or_whitespace_when_native_bytea_is_requested() {
    // Arrange
    let sql = "SELECT decode('é','escape') AS v";
    // Act
    let batches = planning::pruned("bytea-codec-gap-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 99, 51, 97, 57]],
            &[&[0, 1, 0, 0, 0, 2, 195, 169]],
        ],
    );
}

#[test]
fn should_match_codec_edge_1_given_unicode_or_whitespace_when_native_bytea_is_requested() {
    // Arrange
    let sql = "SELECT encode('\\xc3a9'::BYTEA,'escape') AS v";
    // Act
    let batches = planning::pruned("bytea-codec-gap-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 51, 48, 51, 92, 50, 53, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 51, 48, 51, 92, 50, 53, 49]],
        ],
    );
}

#[test]
fn should_match_codec_edge_2_given_unicode_or_whitespace_when_native_bytea_is_requested() {
    // Arrange
    let sql = "SELECT decode('00\tff\n01\r02','hex') AS v";
    // Act
    let batches = planning::pruned("bytea-codec-gap-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 102, 102, 48, 49, 48, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 255, 1, 2]],
        ],
    );
}

#[test]
fn should_match_codec_edge_3_given_unicode_or_whitespace_when_native_bytea_is_requested() {
    // Arrange
    let sql = "SELECT decode('A\tA\r=\n=','base64') AS v";
    // Act
    let batches = planning::pruned("bytea-codec-gap-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_preserve_codec_errors_given_misplaced_whitespace_or_unicode_when_bind_evaluates_decode() {
    // Arrange
    let cases = [
        ("SELECT decode('0 0','hex') AS v", "22023"),
        ("SELECT decode('é','base64') AS v", "22023"),
    ];
    // Act
    let batches = planning::failures("bytea-codec-error-gaps", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_resolve_declared_names_given_bytea_builtin_collisions_when_schema_is_explicit() {
    // Arrange
    let setup = [
        "CREATE SCHEMA bytea_functions_scope",
        "CREATE FUNCTION bytea_functions_scope.encode(x TEXT) RETURNS INT AS \"7\"",
        "CREATE FUNCTION bytea_functions_scope.lower(x INT) RETURNS INT AS \"x + 7\"",
    ];
    // Act
    let encode = planning::pruned_with_setup(
        "bytea-function-declared-encode",
        "SELECT bytea_functions_scope.encode('text'::TEXT) AS v",
        &setup,
    );
    let lower = planning::pruned_with_setup(
        "bytea-function-declared-lower",
        "SELECT bytea_functions_scope.lower(2) AS v",
        &setup,
    );
    // Assert
    support_values_casts::assert_result(
        &encode,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
    support_values_casts::assert_result(
        &lower,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 57]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 9]],
        ],
    );
}

#[test]
fn should_resolve_declared_signature_given_builtin_name_when_search_path_selects_its_schema() {
    // Arrange
    let setup = [
        "CREATE SCHEMA bytea_functions_scope",
        "CREATE FUNCTION bytea_functions_scope.encode(x TEXT) RETURNS INT AS \"7\"",
        "SET search_path TO bytea_functions_scope, pg_catalog",
    ];
    // Act
    let batches = planning::pruned_with_setup(
        "bytea-function-declared-search-path",
        "SELECT encode('text'::TEXT) AS v",
        &setup,
    );
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_keep_builtin_signature_given_different_declared_arity_when_search_path_contains_both() {
    // Arrange
    let setup = [
        "CREATE SCHEMA bytea_functions_scope",
        "CREATE FUNCTION bytea_functions_scope.encode(x TEXT) RETURNS INT AS \"7\"",
        "SET search_path TO bytea_functions_scope, pg_catalog",
    ];
    // Act
    let batches = planning::pruned_with_setup(
        "bytea-function-distinct-arity",
        "SELECT encode('\\x00ff'::BYTEA,'hex') AS v",
        &setup,
    );
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 48, 48, 102, 102]],
            &[&[0, 1, 0, 0, 0, 4, 48, 48, 102, 102]],
        ],
    );
}
