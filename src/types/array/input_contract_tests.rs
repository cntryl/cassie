use super::parse_text_array;
use crate::types::DataType;

#[test]
fn should_reject_text_array_integer_values_outside_declared_domain() {
    // Arrange
    let cases = [
        (DataType::SmallInt, "{40000}"),
        (DataType::SmallInt, "{-32769}"),
        (DataType::SmallInt, "[32768]"),
        (DataType::SmallInt, "[-32769]"),
        (DataType::Int, "{2147483648}"),
        (DataType::Int, "{-2147483649}"),
        (DataType::Int, "[2147483648]"),
        (DataType::BigInt, "[9223372036854775808]"),
    ];

    // Act
    let results = cases.map(|(data_type, input)| parse_text_array(input, &data_type));

    // Assert
    assert!(results.into_iter().all(|result| result.is_err()));
}

#[test]
fn should_reject_json_array_lanes_outside_declared_family() {
    // Arrange
    let cases = [
        (DataType::SmallInt, "[true]"),
        (DataType::Int, "[1.5]"),
        (DataType::BigInt, r#"["1"]"#),
        (DataType::Float, r#"["1"]"#),
        (DataType::Boolean, "[1]"),
        (DataType::Text, "[true]"),
        (DataType::Char { length: None }, "[1]"),
        (DataType::Varchar { length: None }, "[{}]"),
        (DataType::Uuid, "[1]"),
        (DataType::Bytea, "[{}]"),
        (DataType::Date, "[false]"),
        (DataType::Time, "[[null]]"),
        (DataType::Timestamp, "[5]"),
    ];

    // Act
    let results = cases.map(|(data_type, input)| parse_text_array(input, &data_type));

    // Assert
    assert!(results.into_iter().all(|result| result.is_err()));
}

#[test]
fn should_canonicalize_text_array_scalar_spellings() {
    // Arrange
    let cases = [
        (
            DataType::Uuid,
            r#"{"123E4567E89B12D3A456426614174000",NULL}"#,
            "123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            DataType::Uuid,
            r#"["123E4567E89B12D3A456426614174000",null]"#,
            "123e4567-e89b-12d3-a456-426614174000",
        ),
        (DataType::Bytea, r#"{"\\xABcd",NULL}"#, "\\xabcd"),
        (DataType::Bytea, r#"["\\xABcd",null]"#, "\\xabcd"),
        (DataType::Time, r#"{"12:34:56.12",NULL}"#, "12:34:56.120000"),
        (DataType::Time, r#"["12:34:56.12",null]"#, "12:34:56.120000"),
        (
            DataType::Timestamp,
            r#"{"2000-01-01T01:30:00+01:30",NULL}"#,
            "2000-01-01T00:00:00.000000Z",
        ),
        (
            DataType::Timestamp,
            r#"["2000-01-01T01:30:00+01:30",null]"#,
            "2000-01-01T00:00:00.000000Z",
        ),
    ];

    // Act
    let results =
        cases.map(|(data_type, input, expected)| (parse_text_array(input, &data_type), expected));

    // Assert
    for (result, expected) in results {
        assert_eq!(
            result.expect("canonical array"),
            serde_json::json!([expected, null])
        );
    }
}

#[test]
fn should_reject_invalid_string_backed_array_elements() {
    // Arrange
    let cases = [
        (DataType::Uuid, "{bad}"),
        (DataType::Uuid, r#"["bad"]"#),
        (DataType::Bytea, r#"["\\xgg"]"#),
        (DataType::Date, r#"["2000-13-01"]"#),
        (DataType::Time, r#"["24:00:00"]"#),
        (DataType::Timestamp, r#"["bad"]"#),
    ];

    // Act
    let results = cases.map(|(data_type, input)| parse_text_array(input, &data_type));

    // Assert
    assert!(results.into_iter().all(|result| result.is_err()));
}

#[test]
fn should_preserve_declared_numeric_array_precision() {
    // Arrange
    let cases = [
        (
            DataType::BigInt,
            "[9007199254740993,null]",
            serde_json::json!([9_007_199_254_740_993_i64, null]),
        ),
        (
            DataType::BigInt,
            "{9007199254740993,NULL}",
            serde_json::json!([9_007_199_254_740_993_i64, null]),
        ),
        (
            DataType::Float,
            "[9007199254740993,null]",
            serde_json::json!([9_007_199_254_740_992.0_f64, null]),
        ),
        (
            DataType::Float,
            "{9007199254740993,NULL}",
            serde_json::json!([9_007_199_254_740_992.0_f64, null]),
        ),
    ];

    // Act
    let results =
        cases.map(|(data_type, input, expected)| (parse_text_array(input, &data_type), expected));

    // Assert
    for (result, expected) in results {
        assert_eq!(result.expect("declared numeric array"), expected);
    }
}

#[test]
fn should_preserve_float_array_decimal_token_bits() {
    // Arrange
    let cases = [
        ("51.248178375505404", 0x4049_9fc4_4f1b_2f60),
        ("-93.31137037688033", 0xc057_53ed_7e04_693b),
        ("-36.573994842753436", 0xc042_4978_a9ba_d96e),
    ];
    let inputs = cases.into_iter().flat_map(|(token, expected)| {
        [format!("{{{token},NULL}}"), format!("[{token},null]")].map(move |input| (input, expected))
    });

    // Act
    let results = inputs
        .map(|(input, expected)| (parse_text_array(&input, &DataType::Float), expected))
        .collect::<Vec<_>>();

    // Assert
    for (result, expected) in results {
        let value = result.expect("finite FLOAT array");
        assert_eq!(value[0].as_f64().expect("FLOAT lane").to_bits(), expected);
        assert!(value[1].is_null());
    }
}

#[test]
fn should_preserve_finite_float_array_token_boundaries() {
    // Arrange
    let cases = [
        ("1.7976931348623157e308", 0x7fef_ffff_ffff_ffff),
        ("-1.7976931348623157e308", 0xffef_ffff_ffff_ffff),
        ("2.2250738585072014e-308", 0x0010_0000_0000_0000),
        ("5e-324", 1),
        ("-0.0", 0x8000_0000_0000_0000),
        ("1e-400", 0),
        ("1.7976931348623158e308", 0x7fef_ffff_ffff_ffff),
        ("2.4703282292062328e-324", 1),
    ];
    let invalid = [
        "[1e400]",
        r#"["51.248178375505404"]"#,
        "[true]",
        "[{}]",
        "[[]]",
        "[51.248178375505404,]",
        "[51.248178375505404] trailing",
        "[51.248178375505404,null",
    ];

    // Act
    let valid_results = cases
        .into_iter()
        .flat_map(|(token, expected)| {
            [format!("{{{token},NULL}}"), format!("[{token},null]")]
                .map(move |input| (parse_text_array(&input, &DataType::Float), expected))
        })
        .collect::<Vec<_>>();
    let invalid_results = invalid.map(|input| parse_text_array(input, &DataType::Float));

    // Assert
    for (result, expected) in valid_results {
        let value = result.expect("finite FLOAT endpoint");
        assert_eq!(value[0].as_f64().expect("FLOAT lane").to_bits(), expected);
        assert!(value[1].is_null());
    }
    assert!(invalid_results.into_iter().all(|result| result.is_err()));
}

#[test]
fn should_preserve_integer_array_lexical_negative_zero() {
    // Arrange
    let types = [DataType::SmallInt, DataType::Int, DataType::BigInt];
    let valid = ["{-0,NULL}", "[-0,null]"];
    let invalid = ["[-0.0]", "[0.0]", "[0e0]"];

    // Act
    let results = types.map(|data_type| {
        (
            valid.map(|input| parse_text_array(input, &data_type)),
            invalid.map(|input| parse_text_array(input, &data_type)),
        )
    });

    // Assert
    for (valid, invalid) in results {
        for value in valid {
            assert_eq!(
                value.expect("integer negative zero"),
                serde_json::json!([0, null])
            );
        }
        assert!(invalid.into_iter().all(|result| result.is_err()));
    }
}

#[test]
fn should_validate_known_array_character_modifiers() {
    // Arrange
    let character = DataType::Char { length: Some(2) };
    let varchar = DataType::Varchar { length: Some(2) };

    // Act
    let character_value = parse_text_array(r#"["éA  ",null]"#, &character);
    let varchar_value = parse_text_array(r#"["éA",null]"#, &varchar);
    let character_overflow = parse_text_array(r#"["éAB"]"#, &character);
    let varchar_overflow = parse_text_array(r#"{"éAB"}"#, &varchar);

    // Assert
    assert_eq!(
        character_value.expect("known CHAR"),
        serde_json::json!(["éA", null])
    );
    assert_eq!(
        varchar_value.expect("known VARCHAR"),
        serde_json::json!(["éA", null])
    );
    assert!(character_overflow.is_err());
    assert!(varchar_overflow.is_err());
}

#[test]
fn should_preserve_array_character_input_without_wire_modifiers() {
    // Arrange
    let input = r#"{"éA  ",NULL}"#;

    // Act
    let character = parse_text_array(input, &DataType::Char { length: None });
    let varchar = parse_text_array(input, &DataType::Varchar { length: None });

    // Assert
    assert_eq!(
        character.expect("unknown CHAR modifier"),
        serde_json::json!(["éA  ", null])
    );
    assert_eq!(
        varchar.expect("unlimited VARCHAR"),
        serde_json::json!(["éA  ", null])
    );
}
