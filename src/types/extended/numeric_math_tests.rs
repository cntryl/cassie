use super::super::{parse_text, Numeric};
use super::{divide, exp, ln, log, modulo, multiply, power, round, sqrt, trunc};

#[test]
fn should_report_numeric_rounding_carry_overflow() {
    // Arrange
    // A format-boundary value requires all 131,072 integer digits; this is the
    // smallest precision that can exercise PostgreSQL's numeric weight limit.
    let magnitude = num_bigint::BigInt::from(10_u8)
        .pow(u32::try_from(super::super::MAX_INTEGER_DIGITS + 1).expect("format limit"))
        - 9_u8;
    let positive = Numeric::finite(magnitude.clone(), 1);
    let negative = Numeric::finite(-magnitude, 1);

    // Act
    let ceiling = super::ceil_checked(&positive);
    let floor = super::floor_checked(&negative);
    let floor_control = super::floor_checked(&positive);
    let ceiling_control = super::ceil_checked(&negative);

    // Assert
    assert_eq!(
        ceiling.expect_err("positive carry exceeds format").sqlstate,
        "22003"
    );
    assert_eq!(
        floor.expect_err("negative carry exceeds format").sqlstate,
        "22003"
    );
    assert_eq!(
        floor_control
            .expect("positive floor remains in range")
            .integer_digits(),
        super::super::MAX_INTEGER_DIGITS
    );
    assert_eq!(
        ceiling_control
            .expect("negative ceiling remains in range")
            .integer_digits(),
        super::super::MAX_INTEGER_DIGITS
    );
}

fn numeric(text: &str) -> Numeric {
    parse_text(text).expect("valid numeric input")
}

#[test]
fn should_reject_finite_numeric_exponential_overflow_before_fixed_point_construction() {
    // Arrange
    let cases = ["6000", "6000.00", "1e400"];

    // Act
    let results = cases.map(|input| exp(&numeric(input)));

    // Assert
    for result in results {
        assert_eq!(result.expect_err("PostgreSQL EXP bound").sqlstate, "22003");
    }
}

#[test]
fn should_preserve_exponential_zero_identity_given_finite_underflow_or_infinity() {
    // Arrange
    let cases = ["-6000", "-1e400"];
    let expected_zero = format!("0.{}", "0".repeat(1000));

    // Act
    let results = cases.map(|input| exp(&numeric(input)).expect("finite underflow").to_string());
    let infinity_zero = exp(&numeric("-Infinity")).expect("negative infinity");

    // Assert
    assert_eq!(results, [expected_zero.clone(), expected_zero]);
    assert_eq!(infinity_zero.to_string(), "0");
}

#[test]
fn should_preserve_integer_power_range_when_bounded_workspace_uses_an_exponential_fallback() {
    // Arrange
    let base = numeric(&format!("10.{}", "0".repeat(100)));
    let exponent = numeric("3000");
    let expected = format!("1{}.{}", "0".repeat(3000), "0".repeat(100));

    // Act
    let integer_result = power(&base, &exponent).expect("INT32 exponent range");
    let general_result = power(&numeric("1e4"), &numeric("1000.5"));

    // Assert
    assert_eq!(integer_result.to_string(), expected);
    assert_eq!(
        general_result
            .expect_err("general exponential bound")
            .sqlstate,
        "22003"
    );
}

#[test]
fn should_preserve_all_integer_digits_of_a_supported_large_numeric_exponential() {
    // Arrange
    let expected = include_str!("../../../tests/fixtures/math/exp-5000-pg18.6.txt").trim();
    let input = numeric("5000");

    // Act
    let result = exp(&input).expect("supported finite exponential");

    // Assert
    assert_eq!(result.to_string(), expected);
}

#[test]
fn should_refine_power_precision_after_an_extreme_exponent_amplifies_a_near_unity_base() {
    // Arrange
    let base = numeric(&format!("1.{}1", "0".repeat(399)));
    let cases = [
        (
            "1e403",
            include_str!("../../../tests/fixtures/math/power-near-one-positive-pg18.6.txt"),
        ),
        (
            "-1e403",
            include_str!("../../../tests/fixtures/math/power-near-one-negative-pg18.6.txt"),
        ),
    ];

    // Act
    let results = cases.map(|(exponent, _)| {
        power(&base, &numeric(exponent))
            .expect("finite power")
            .to_string()
    });

    // Assert
    assert_eq!(results, cases.map(|(_, expected)| expected.trim()));
}

#[test]
fn should_preserve_reciprocal_identity_in_the_bounded_integer_power_fallback() {
    // Arrange
    let magnitude = format!("10.{}", "0".repeat(1000));
    let cases = [
        (
            magnitude.clone(),
            "-200",
            format!("0.{}1{}", "0".repeat(199), "0".repeat(800)),
        ),
        (
            format!("-{magnitude}"),
            "-201",
            format!("-0.{}1{}", "0".repeat(200), "0".repeat(799)),
        ),
    ];

    // Act
    let results = cases.each_ref().map(|(base, exponent, _)| {
        power(&numeric(base), &numeric(exponent))
            .expect("finite reciprocal")
            .to_string()
    });

    // Assert
    assert_eq!(results, cases.map(|(_, _, expected)| expected));
}

#[test]
fn should_apply_the_upstream_float8_guard_to_corrected_near_boundary_exponential_arguments() {
    // Arrange
    let near_one = numeric(&format!("1.{}1", "0".repeat(399)));
    let below_but_rounded_to_boundary = numeric("5999.99999999999999999999");

    // Act
    let direct = exp(&below_but_rounded_to_boundary);
    let general = power(&near_one, &numeric("6e403"));

    // Assert
    assert_eq!(
        direct.expect_err("upstream FLOAT8 boundary").sqlstate,
        "22003"
    );
    assert_eq!(
        general.expect_err("corrected product boundary").sqlstate,
        "22003"
    );
}

#[test]
fn should_reject_undefined_numeric_powers_with_infinite_operands() {
    // Arrange
    let cases = [("-Infinity", "0.5"), ("0", "-Infinity")];

    for (base, exponent) in cases {
        // Act
        let result = power(&numeric(base), &numeric(exponent));

        // Assert
        assert_eq!(
            result
                .expect_err("PostgreSQL numeric power domain error")
                .sqlstate,
            "2201F",
            "power({base}, {exponent})"
        );
    }
}

#[test]
fn should_preserve_exact_signs_for_infinite_numeric_powers() {
    // Arrange
    let cases = [
        ("-Infinity", "9007199254740993", "-Infinity"),
        ("-Infinity", "9007199254740992", "Infinity"),
        ("-Infinity", "3", "-Infinity"),
        ("-Infinity", "-3", "0"),
        ("-1", "Infinity", "1"),
        ("0.5", "-Infinity", "Infinity"),
        ("2", "-Infinity", "0"),
        ("1", "NaN", "1"),
        ("NaN", "0", "1"),
    ];

    for (base, exponent, expected) in cases {
        // Act
        let result = power(&numeric(base), &numeric(exponent));

        // Assert
        assert_eq!(text(result), expected, "power({base}, {exponent})");
    }
}

#[test]
fn should_evaluate_negative_numeric_bases_with_large_integral_exponents() {
    // Arrange
    let cases = [
        ("-1", "2147483648", "1.0000000000000000"),
        ("-1", "9007199254740993", "-1.0000000000000000"),
        ("-2", "2147483648", "22003"),
    ];

    for (base, exponent, expected) in cases {
        // Act
        let result = power(&numeric(base), &numeric(exponent));

        // Assert
        assert_eq!(text(result), expected, "power({base}, {exponent})");
    }
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

#[test]
fn should_preserve_numeric_powers_for_bases_outside_float8_range() {
    // Arrange
    let cases = [
        ("1e400","0.001","2.5118864315095801"),
        ("1e-400","0.001","0.3981071705534972507702523050877520434876770372973804468652841480602248538694580390740183116073773602535776139555600987359459931559542153889451122557074494399602978580749066768664769973604975136104212100292796272931177158291538247427232938675773982983408478860432348749664956468291027150701465206652900930163192878085824785328617411517663410579620331388841815858927107478346934543248628397714301824926"),
        ("1e-1021","0.0001","0.7904965884764858966217178751524682685594802961802002005851930071224963198072323659293346684215993580623577213411850014358741862630994528423082951560626970861039362370835317984180143882826452163018101679035006092749045625327733262281858108023863437418293883829965276690957061221955330162831487018296257096376762222671276224794258228799135851093412888434593939315845398200032485359379041476702023271057676386544291339721839572899723149330262595634238889343261619324868645838017187094058594967535409112659667103917666282719632039231398089718542670109701270637963641503391762558805996927816043942777373457285618128412552216749067983937586256791872793195499190747374606498255916691383699742848822614300023296104421876903521643048082103503957114175371394730647349278006782055534265377266791988604632690764264401201238278045705199331640624559999969628976887915676956252131229171980800613029722771071914904529543314660287136321873918100664061675159115052862099458296859054689276023201238059449930307774918314"),
    ];

    // Act
    let actual = cases.map(|(base, exponent, _)| text(power(&numeric(base), &numeric(exponent))));

    // Assert
    assert_eq!(actual, cases.map(|(_, _, expected)| expected));
}

#[test]
fn should_evaluate_large_numeric_powers_without_expanding_unbounded_coefficients() {
    // Arrange
    let cases = [
        ("1.0000000","2147483647","1.0000000000000000"),
        ("1.0000001","2147483647","1836644589403938098389174471618404689695459148838905933332308371072921650416957808636122148027.4088621"),
        ("0.0000000","2147483647","0.0000000000000000"),
    ];

    // Act
    let actual = cases.map(|(base, exponent, _)| text(power(&numeric(base), &numeric(exponent))));

    // Assert
    assert_eq!(actual, cases.map(|(_, _, expected)| expected));
}

#[test]
fn should_preserve_near_unity_numeric_powers_with_large_integral_exponents() {
    // Arrange
    let cases = [
        (
            "1.00000000000000000001",
            "100000000000000000000",
            "2.71828182845904523535",
        ),
        (
            "0.99999999999999999999",
            "100000000000000000000",
            "0.36787944117144232159",
        ),
        (
            "-1.00000000000000000001",
            "100000000000000000001",
            "-2.71828182845904523537",
        ),
    ];

    // Act
    let actual = cases.map(|(base, exponent, _)| text(power(&numeric(base), &numeric(exponent))));

    // Assert
    assert_eq!(actual, cases.map(|(_, _, expected)| expected));
}
