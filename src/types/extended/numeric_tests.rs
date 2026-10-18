use super::{parse_text, parse_type_name, receive_binary, Numeric, NumericTypmod};

fn numeric(text: &str) -> Numeric {
    parse_text(text).expect("valid numeric input")
}

#[test]
fn should_format_numeric_input_with_its_display_scale() {
    // Arrange
    let inputs = [
        "1.50e1", ".5", "5.", "-0.00", " -inf ", "1_000.5", "0x1F", "1.5e-3", "0.000e5", "NaN",
    ];

    // Act
    let formatted = inputs.map(|input| numeric(input).to_string());

    // Assert
    assert_eq!(
        formatted,
        [
            "15.0",
            "0.5",
            "5",
            "0.00",
            "-Infinity",
            "1000.5",
            "31",
            "0.0015",
            "0",
            "NaN"
        ]
    );
}

#[test]
fn should_reject_numeric_input_with_postgresql_sqlstates() {
    // Arrange
    let inputs = [
        "+NaN", "1e", "0x", "abc", "1e131072", "1e-16384", "0e-20000",
    ];

    // Act
    let states = inputs.map(|input| parse_text(input).map_err(|error| error.sqlstate));

    // Assert
    assert_eq!(
        states,
        [
            Err("22P02"),
            Err("22P02"),
            Err("22P02"),
            Err("22P02"),
            Err("22003"),
            Err("22003"),
            Err("22003")
        ]
    );
}

#[test]
fn should_round_numeric_typmods_half_away_from_zero() {
    // Arrange
    let typmod = NumericTypmod::new(5, 2).expect("valid typmod");
    let negative = NumericTypmod::new(2, -3).expect("valid typmod");

    // Act
    let rounded = [
        numeric("1.245").apply_typmod(Some(typmod)),
        numeric("-1.245").apply_typmod(Some(typmod)),
        numeric("12500").apply_typmod(Some(negative)),
        numeric("12345.678").apply_typmod(Some(typmod)),
        numeric("Infinity").apply_typmod(Some(typmod)),
    ]
    .map(|value| {
        value
            .map(|value| value.to_string())
            .map_err(|error| error.sqlstate)
    });

    // Assert
    assert_eq!(
        rounded,
        [
            Ok("1.25".to_string()),
            Ok("-1.25".to_string()),
            Ok("13000".to_string()),
            Err("22003"),
            Err("22003")
        ]
    );
}

#[test]
fn should_order_numeric_values_with_nan_last_regardless_of_scale() {
    // Arrange
    let mut values = ["NaN", "Infinity", "1.00", "-Infinity", "0.5", "-2", "1"].map(numeric);

    // Act
    values.sort_by_key(Numeric::order_key);

    // Assert
    assert_eq!(numeric("1.0").order_key(), numeric("1.000").order_key());
    assert_eq!(
        values.map(|value| value.to_string()),
        ["-Infinity", "-2", "0.5", "1.00", "1", "Infinity", "NaN"]
    );
}

#[test]
fn should_round_trip_numeric_binary_frames() {
    // Arrange
    let values = [
        "18014398509481985",
        "9007199254740992.5000",
        "-0.00012",
        "0.000",
        "NaN",
    ];

    // Act
    let decoded = values
        .map(|value| receive_binary(&numeric(value).send_binary()).map(|value| value.to_string()));

    // Assert
    assert_eq!(decoded, values.map(|value| Ok(value.to_string())));
    assert_eq!(
        numeric("18014398509481985").send_binary(),
        [0, 5, 0, 4, 0, 0, 0, 0, 0, 1, 0x1f, 0x4e, 0x0f, 0x91, 0x03, 0xb4, 0x07, 0xc1]
    );
}

#[test]
fn should_parse_numeric_type_modifiers_with_postgresql_limits() {
    // Arrange
    let names = [
        "numeric",
        "decimal(5, 2)",
        "numeric(5)",
        "numeric(1001)",
        "numeric(5,-1001)",
    ];

    // Act
    let parsed = names.map(|name| parse_type_name(name).expect("numeric spelling"));

    // Assert
    assert_eq!(
        parsed,
        [
            Ok(None),
            Ok(Some(NumericTypmod {
                precision: 5,
                scale: 2
            })),
            Ok(Some(NumericTypmod {
                precision: 5,
                scale: 0
            })),
            Err("NUMERIC precision 1001 must be between 1 and 1000".to_string()),
            Err("NUMERIC scale -1001 must be between -1000 and 1000".to_string())
        ]
    );
}

#[test]
fn should_reject_fractional_values_above_a_scale_larger_than_precision() {
    // Arrange
    let typmod = NumericTypmod::new(2, 5).expect("valid typmod");

    // Act
    let results = ["0.000994", "0.00100"].map(|input| {
        numeric(input)
            .apply_typmod(Some(typmod))
            .map(|value| value.to_string())
            .map_err(|error| error.sqlstate)
    });

    // Assert
    assert_eq!(results, [Ok("0.00099".to_string()), Err("22003")]);
}
