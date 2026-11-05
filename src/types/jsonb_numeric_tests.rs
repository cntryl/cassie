use super::{canonical, input_text_len, text_len};

#[test]
fn should_bound_jsonb_numeric_weight_from_significant_digits() {
    // Arrange
    let input = r#"{"n":0.01e131073}"#;
    let document = serde_json::from_str(input).expect("exact lexical JSON");
    // Act
    let text_bound = input_text_len(input).expect("valid format bound");
    let value_bound = text_len(&document).expect("borrowed number bound");
    let rendered = canonical(input).expect("canonical exact boundary");
    // Assert
    assert_eq!(rendered.len(), 131079);
    assert!(text_bound >= rendered.len());
    assert!(value_bound >= rendered.len());
}

#[test]
fn should_bound_zero_jsonb_numbers_without_expanding_positive_exponents() {
    // Arrange
    let cases = ["0e1073741823", "-0e1073741823", "0.00e1000000"];
    // Act
    let bounds = cases.map(|text| input_text_len(text).expect("zero bound"));
    let values = cases.map(|text| canonical(text).expect("canonical zero"));
    // Assert
    assert_eq!(values, ["0", "0", "0"]);
    assert!(bounds.iter().all(|bound| *bound < 64));
}

#[test]
fn should_reject_true_jsonb_numeric_format_errors_during_pre_admission() {
    // Arrange
    let cases = ["0.01e131074", "0e1073741824", "0.00e-16382", "[1e131072,]"];
    // Act
    let errors = cases.map(|text| input_text_len(text).expect_err("true numeric format overflow"));
    // Assert
    assert!(errors.iter().all(|error| error.sqlstate == "22003"));
}

#[test]
fn should_preserve_malformed_jsonb_numeric_token_errors_during_pre_admission() {
    // Arrange
    let cases = ["1e131072x", "0000.01e131073", "1e+", "0.01e131073x"];
    // Act
    let errors = cases.map(|text| input_text_len(text).expect_err("invalid lexical number"));
    // Assert
    assert!(errors.iter().all(|error| error.sqlstate == "22P02"));
}

#[path = "jsonb_numeric_cases.rs"]
mod fixture;

#[test]
fn should_cover_pinned_jsonb_output_bytes_across_decimal_weight_regimes() {
    // Arrange
    let cases = fixture::CASES;
    // Act
    let values = cases
        .iter()
        .map(|(input, _)| {
            let parsed = serde_json::from_str(input).expect("JSON number");
            (
                canonical(input).expect("canonical numeric"),
                input_text_len(input).expect("input bound"),
                text_len(&parsed).expect("value bound"),
            )
        })
        .collect::<Vec<_>>();
    // Assert
    for ((input, expected), (actual, input_bound, value_bound)) in cases.iter().zip(values) {
        assert_eq!(&actual, expected, "{input}");
        assert!(input_bound >= actual.len(), "{input}");
        assert!(value_bound >= actual.len(), "{input}");
    }
}
