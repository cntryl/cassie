use super::{fixture, wire};

type Frames = Vec<(u8, Vec<u8>)>;

fn execute(name: &str, sql: &str) -> Vec<Frames> {
    fixture::run_cycles(name, &[], vec![fixture::execute_cycle(sql, None, 0, true)])
}

fn assert_truths(batches: &[Frames], expected: &[Option<&str>]) {
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![expected
            .iter()
            .map(|value| value.map(str::to_owned))
            .collect::<Vec<_>>()]
    );
}

#[test]
fn should_compare_exact_jsonb_numbers_given_adjacent_wide_decimals_when_equality_is_evaluated() {
    // Arrange
    let sql = "SELECT '123456789012345678901234567890.125'::JSONB = '123456789012345678901234567890.126'::JSONB, '2.00'::JSONB = '2'::JSONB";
    // Act
    let batches = execute("jsonb-exact-numeric-equality", sql);
    // Assert
    assert_truths(&batches, &[Some("f"), Some("t")]);
}

#[test]
fn should_order_jsonb_numbers_given_numeric_magnitudes_when_comparisons_are_evaluated() {
    // Arrange
    let sql = "SELECT '2'::JSONB < '10'::JSONB, '-10'::JSONB < '-2'::JSONB, '1.01'::JSONB > '1.001'::JSONB";
    // Act
    let batches = execute("jsonb-numeric-order", sql);
    // Assert
    assert_truths(&batches, &[Some("t"), Some("t"), Some("t")]);
}

#[test]
fn should_order_jsonb_containers_given_different_sizes_when_comparisons_are_evaluated() {
    // Arrange
    let sql = "SELECT '[2]'::JSONB < '[1,0]'::JSONB, '{\"z\":1}'::JSONB < '{\"a\":1,\"b\":2}'::JSONB, '[1,2]'::JSONB < '[1,3]'::JSONB";
    // Act
    let batches = execute("jsonb-container-order", sql);
    // Assert
    assert_truths(&batches, &[Some("t"), Some("t"), Some("t")]);
}

#[test]
fn should_order_jsonb_shapes_given_document_null_when_comparisons_are_evaluated() {
    // Arrange
    let sql = "SELECT '[]'::JSONB < 'null'::JSONB, 'null'::JSONB < '\"text\"'::JSONB, '\"text\"'::JSONB < '1'::JSONB, '1'::JSONB < 'false'::JSONB, 'true'::JSONB < '[1]'::JSONB, '[1]'::JSONB < '{}'::JSONB";
    // Act
    let batches = execute("jsonb-shape-order", sql);
    // Assert
    assert_truths(&batches, &[Some("t"); 6]);
}

#[test]
fn should_preserve_jsonb_comparison_nulls_given_sql_null_when_distinctness_is_evaluated() {
    // Arrange
    let sql = "SELECT NULL::JSONB < '1'::JSONB, NULL::JSONB IS DISTINCT FROM 'null'::JSONB, '2.00'::JSONB IS NOT DISTINCT FROM '2'::JSONB";
    // Act
    let batches = execute("jsonb-null-comparisons", sql);
    // Assert
    assert_truths(&batches, &[None, Some("t"), Some("t")]);
}

#[test]
fn should_resolve_jsonb_comparison_literals_given_unknown_operands_when_binding_overloads() {
    // Arrange
    let sql = "SELECT '2'::JSONB < '10', '10' > '2'::JSONB, '2'::JSONB = '2.0'";
    // Act
    let batches = execute("jsonb-unknown-comparison-operands", sql);
    // Assert
    assert_truths(&batches, &[Some("t"), Some("t"), Some("t")]);
}

#[test]
fn should_reject_jsonb_comparison_types_given_explicit_nonjsonb_operands_when_binding_overloads() {
    // Arrange
    let cycles = [
        "SELECT '2'::JSONB < '10'::TEXT",
        "SELECT '2'::JSONB = '2'::JSON",
    ]
    .map(|sql| fixture::execute_cycle(sql, None, 0, true))
    .to_vec();
    // Act
    let batches = fixture::run_cycles("jsonb-invalid-comparison-types", &[], cycles);
    // Assert
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("42883"));
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_distinguish_root_jsonb_empty_arrays_given_nested_arrays_when_comparisons_are_evaluated() {
    // Arrange
    let sql = "SELECT '[]'::JSONB < 'null'::JSONB, '[[]]'::JSONB > '[null]'::JSONB, '{\"v\":[]}'::JSONB > '{\"v\":null}'::JSONB";
    // Act
    let batches = execute("jsonb-nested-empty-array-order", sql);
    // Assert
    assert_truths(&batches, &[Some("t"), Some("t"), Some("t")]);
}
