use super::super::{parse_text, Numeric};
use super::{divide, exp, ln, log, modulo, multiply, power, round, sqrt, trunc};

fn numeric(text: &str) -> Numeric {
    parse_text(text).expect("valid numeric input")
}

fn text(result: Result<Numeric, super::TypeInputError>) -> String {
    result.map_or_else(
        |error| error.sqlstate.to_string(),
        |value| value.to_string(),
    )
}

#[test]
fn should_divide_with_postgresql_select_div_scale() {
    // Arrange
    let cases = [
        ("1", "3"),
        ("1.5", "3"),
        ("10", "4"),
        ("18014398509481985", "2"),
    ];

    // Act
    let quotients = cases.map(|(left, right)| text(divide(&numeric(left), &numeric(right))));

    // Assert
    assert_eq!(
        quotients,
        [
            "0.33333333333333333333",
            "0.50000000000000000000",
            "2.5000000000000000",
            "9007199254740992.5000"
        ]
    );
}

#[test]
fn should_keep_operand_scales_for_products_with_remainders() {
    // Arrange
    let (left, right) = (numeric("2.5"), numeric("1.25"));

    // Act
    let product = text(multiply(&left, &right));
    let remainders = [("7.5", "2"), ("-7.5", "2"), ("1", "0")]
        .map(|(left, right)| text(modulo(&numeric(left), &numeric(right))));

    // Assert
    assert_eq!(product, "3.125");
    assert_eq!(remainders, ["1.5", "-1.5", "22012"]);
}

#[test]
fn should_match_postgresql_transcendental_result_scales() {
    // Arrange
    let two = numeric("2");

    // Act
    let results = [
        text(sqrt(&two)),
        text(exp(&numeric("1"))),
        text(ln(&two)),
        text(log(&numeric("10"), &numeric("100"))),
        text(power(&two, &numeric("0.5"))),
        text(power(&two, &numeric("10"))),
        text(power(&numeric("9"), &numeric("3"))),
    ];

    // Assert
    assert_eq!(
        results,
        [
            "1.414213562373095",
            "2.7182818284590452",
            "0.6931471805599453",
            "2.0000000000000000",
            "1.4142135623730950",
            "1024.0000000000000",
            "729.00000000000000"
        ]
    );
}

#[test]
fn should_round_half_away_from_zero_beside_truncation_toward_zero() {
    // Arrange
    let values = ["1.245", "-1.245", "1234.5"];

    // Act
    let rounded = values.map(|value| text(round(&numeric(value), 2)));
    let tens = text(round(&numeric("1234.5"), -2));
    let truncated = trunc(&numeric("-125.9"), -1).to_string();

    // Assert
    assert_eq!(rounded, ["1.25", "-1.25", "1234.50"]);
    assert_eq!(tens, "1200");
    assert_eq!(truncated, "-120");
}
