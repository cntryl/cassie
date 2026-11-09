//! PostgreSQL 18 temporal output formatting regressions tracked by #815.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_format_interval_hours_beyond_the_i32_range() {
    // Arrange
    let fixture = sql_fixture("issue-815-interval-wide-hour", &[]);
    let template = "HH24|FMHH24|HH12|HH24th|MI|SS|SSSS|AM";
    let cases = [
        (
            "2147483647 hours",
            "2147483647|2147483647|07|2147483647th|00|00|7730941129200|AM",
        ),
        (
            "2147483648 hours 2 minutes 3.000001 seconds",
            "2147483648|2147483648|08|2147483648th|02|03|7730941132923|AM",
        ),
        (
            "-2147483648 hours",
            "-2147483648|-2147483648|-08|-2147483648th|00|00|-7730941132800|AM",
        ),
        (
            "-2147483649 hours -2 minutes -3.000001 seconds",
            "-2147483649|-2147483649|-09|-2147483649th|-02|-03|-7730941136523|AM",
        ),
        (
            "2562047788 hours",
            "2562047788|2562047788|04|2562047788th|00|00|9223372036800|AM",
        ),
        (
            "-2562047788 hours",
            "-2562047788|-2562047788|-04|-2562047788th|00|00|-9223372036800|AM",
        ),
    ];

    for (interval, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{interval}', '{template}')"
        ));

        // Assert
        assert_eq!(
            result.expect("wide interval hour format").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 wide interval hour: {interval}"
        );
    }

    // Act
    let boundary = fixture.execute("SELECT to_char(INTERVAL '2562047788 hours 54.775806 seconds', 'HH24|MI|SS|US|SSSS'), to_char(INTERVAL '-2562047788 hours -54.775806 seconds', 'HH24|MI|SS|US|SSSS'), to_char(INTERVAL '2147483652 hours', 'HH24|HH12|SSSS|AM'), to_char(INTERVAL '-2147483652 hours', 'HH24|HH12|SSSS|AM'), to_char(TIMESTAMP '2000-01-01 13:00:00', 'HH24|HH12|SSSS|AM'), to_char(INTERVAL 'infinity', 'HH24') IS NULL, to_char(NULL::interval, 'HH24') IS NULL");

    // Assert
    assert_eq!(
        boundary.expect("wide interval hour boundaries").rows(),
        &[vec![
            Value::String("2562047788|00|54|775806|9223372036854".to_owned()),
            Value::String("-2562047788|00|-54|-775806|-9223372036854".to_owned()),
            Value::String("2147483652|12|7730941147200|PM".to_owned()),
            Value::String("-2147483652|012|-7730941147200|AM".to_owned()),
            Value::String("13|01|46800|PM".to_owned()),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_render_interval_twelve_hour_clock_at_multiples_of_twelve() {
    // Arrange
    let fixture = sql_fixture("issue-815-interval-hour12", &[]);
    let cases = [
        (-25, "-01|-1|-01st|-01|-25"),
        (-24, "012|12|012th|012|-24"),
        (-13, "-01|-1|-01st|-01|-13"),
        (-12, "012|12|012th|012|-12"),
        (-11, "-11|-11|-11th|-11|-11"),
        (-1, "-01|-1|-01st|-01|-01"),
        (0, "12|12|12th|12|00"),
        (1, "01|1|01st|01|01"),
        (11, "11|11|11th|11|11"),
        (12, "12|12|12th|12|12"),
        (13, "01|1|01st|01|13"),
        (24, "12|12|12th|12|24"),
        (25, "01|1|01st|01|25"),
    ];

    for (hours, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{hours} hours', 'HH12|FMHH12|HH12th|HH|HH24')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval twelve-hour format").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 hour format for {hours} hours"
        );
    }

    // Act
    let timestamp =
        fixture.execute("SELECT to_char(TIMESTAMP '2000-01-01 00:00:00', 'HH12|FMHH12|HH24')");

    // Assert
    assert_eq!(
        timestamp.expect("timestamp twelve-hour format").rows(),
        &[vec![Value::String("12|12|00".to_owned())]]
    );
}

#[test]
fn should_keep_fraction_width_for_fill_mode_and_negative_intervals() {
    // Arrange
    let fixture = sql_fixture("issue-815-fraction-format-width", &[]);
    let template =
        "MS|FMMS|MSth|US|FMUS|USth|FF1|FMFF1|FF2|FMFF2|FF3|FMFF3|FF4|FMFF4|FF5|FMFF5|FF6|FMFF6";
    let cases = [
        ("0.005006", "005|005|005th|005006|005006|005006th|0|0|00|00|005|005|0050|0050|00500|00500|005006|005006"),
        ("-0.005006", "-05|-05|-05th|-05006|-05006|-05006th|0|0|00|00|-05|-05|-050|-050|-0500|-0500|-05006|-05006"),
        ("0.000001", "000|000|000th|000001|000001|000001st|0|0|00|00|000|000|0000|0000|00000|00000|000001|000001"),
        ("-0.000001", "000|000|000th|-00001|-00001|-00001st|0|0|00|00|000|000|0000|0000|00000|00000|-00001|-00001"),
        ("0.500005", "500|500|500th|500005|500005|500005th|5|5|50|50|500|500|5000|5000|50000|50000|500005|500005"),
        ("-0.500005", "-500|-500|-500th|-500005|-500005|-500005th|-5|-5|-50|-50|-500|-500|-5000|-5000|-50000|-50000|-500005|-500005"),
    ];

    for (seconds, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{seconds} seconds', '{template}')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval fractional format").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 fraction: {seconds}"
        );
    }

    // Act
    let timestamp = fixture.execute(
        "SELECT to_char(TIMESTAMP '2000-01-01 00:00:00.005006', 'MS|FMMS|US|FMUS|FF3|FMFF3')",
    );

    // Assert
    assert_eq!(
        timestamp.expect("timestamp fractional format").rows(),
        &[vec![Value::String(
            "005|005|005006|005006|005|005".to_owned()
        )]]
    );
}

#[test]
fn should_format_interval_day_of_year_and_week_from_approximate_span() {
    // Arrange
    let fixture = sql_fixture("issue-815-interval-day-of-year-week", &[]);
    let cases = [
        ("0", "000|0|000th|01|1|01st"),
        ("1 day", "001|1|001st|01|1|01st"),
        ("1 month", "030|30|030th|05|5|05th"),
        ("1 year 2 months 3 days", "423|423|423rd|61|61|61st"),
        ("-1 day", "-01|-1|-01st|01|1|01st"),
        ("-1 month -3 days", "-33|-33|-33rd|-3|-3|-3rd"),
        ("1 month -30 days", "000|0|000th|01|1|01st"),
        ("1 month -31 days", "-01|-1|-01st|01|1|01st"),
        ("7 days", "007|7|007th|01|1|01st"),
        ("8 days", "008|8|008th|02|2|02nd"),
    ];

    for (interval, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{interval}', 'DDD|FMDDD|DDDth|WW|FMWW|WWth')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval span formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 interval span: {interval}"
        );
    }

    // Act
    let extremes = fixture.execute("SELECT to_char(INTERVAL '2147483647 months', 'DDD|WW'), to_char(INTERVAL '-2147483648 months', 'DDD|WW'), to_char(INTERVAL '2147483647 days', 'DDD|WW'), to_char(INTERVAL '-2147483648 days', 'DDD|WW'), to_char(INTERVAL '2147483647 months 2147483647 days', 'DDD|WW')");
    let date = fixture.execute("SELECT to_char(TIMESTAMP '2024-12-31', 'DDD|WW')");

    // Assert
    assert_eq!(
        extremes.expect("interval span extremes").rows(),
        &[vec![
            Value::String("-30|-3".to_owned()),
            Value::String("000|01".to_owned()),
            Value::String("2147483647|306783379".to_owned()),
            Value::String("-2147483648|306783379".to_owned()),
            Value::String("2147483617|306783374".to_owned()),
        ]]
    );
    assert_eq!(
        date.expect("timestamp day of year and week").rows(),
        &[vec![Value::String("366|53".to_owned())]]
    );
}

#[test]
fn should_return_null_for_an_empty_temporal_to_char_template() {
    // Arrange
    let fixture = sql_fixture("issue-815-empty-temporal-to-char", &[]);

    // Act
    let nulls = fixture.execute(
        "SELECT to_char(TIMESTAMP '2000-01-01', '') IS NULL, to_char(TIMESTAMPTZ '2000-01-01 00:00:00+00', '') IS NULL, to_char(DATE '2000-01-01', '') IS NULL, to_char(INTERVAL '1 day', '') IS NULL, to_char(TIMESTAMP 'infinity', '') IS NULL",
    );
    let quoted_empty = fixture.execute("SELECT to_char(INTERVAL '1 day', '\"\"')");
    let ordinary = fixture.execute("SELECT to_char(TIMESTAMP '2000-01-01', 'YYYY')");

    // Assert
    assert_eq!(
        nulls.expect("empty temporal formats").rows(),
        &[vec![Value::Bool(true); 5]]
    );
    assert_eq!(
        quoted_empty.expect("quoted empty literal").rows(),
        &[vec![Value::String(String::new())]]
    );
    assert_eq!(
        ordinary.expect("nonempty format").rows(),
        &[vec![Value::String("2000".to_owned())]]
    );
}

#[test]
fn should_format_interval_centuries_by_truncating_signed_years() {
    // Arrange
    let fixture = sql_fixture("issue-815-interval-century", &[]);
    let cases = [
        (-300, "-03|-3|-03rd|-03RD"),
        (-201, "-02|-2|-02nd|-02ND"),
        (-200, "-02|-2|-02nd|-02ND"),
        (-199, "-01|-1|-01st|-01ST"),
        (-100, "-01|-1|-01st|-01ST"),
        (-99, "00|0|00th|00TH"),
        (-1, "00|0|00th|00TH"),
        (0, "00|0|00th|00TH"),
        (1, "00|0|00th|00TH"),
        (99, "00|0|00th|00TH"),
        (100, "01|1|01st|01ST"),
        (101, "01|1|01st|01ST"),
        (200, "02|2|02nd|02ND"),
        (201, "02|2|02nd|02ND"),
    ];

    for (years, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{years} years', 'CC|FMCC|CCth|CCTH')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval century formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 century for {years} years"
        );
    }

    // Act
    let dates = fixture.execute("SELECT to_char(TIMESTAMP '0001-01-01', 'CC'), to_char(TIMESTAMP '0001-01-01 BC', 'CC'), to_char(TIMESTAMP '2000-01-01', 'CC')");

    // Assert
    assert_eq!(
        dates.expect("timestamp century formatting").rows(),
        &[vec![
            Value::String("01".to_owned()),
            Value::String("-01".to_owned()),
            Value::String("20".to_owned()),
        ]]
    );
}

#[test]
fn should_count_the_sign_in_negative_interval_day_padding() {
    // Arrange
    let fixture = sql_fixture("issue-815-negative-interval-day-padding", &[]);
    let cases = [
        (-99, "-99|-99|-99th|-99TH"),
        (-21, "-21|-21|-21st|-21ST"),
        (-11, "-11|-11|-11th|-11TH"),
        (-10, "-10|-10|-10th|-10TH"),
        (-9, "-9|-9|-9th|-9TH"),
        (-3, "-3|-3|-3rd|-3RD"),
        (-2, "-2|-2|-2nd|-2ND"),
        (-1, "-1|-1|-1st|-1ST"),
        (0, "00|0|00th|00TH"),
        (1, "01|1|01st|01ST"),
        (9, "09|9|09th|09TH"),
        (10, "10|10|10th|10TH"),
        (99, "99|99|99th|99TH"),
    ];

    for (days, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{days} days', 'DD|FMDD|DDth|DDTH')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval day formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 day formatting for {days} days"
        );
    }
}

#[test]
fn should_render_negative_interval_ordinal_suffixes_from_the_magnitude() {
    // Arrange
    let fixture = sql_fixture("issue-815-negative-interval-ordinal", &[]);
    let cases = [
        (-1, "-01st|-01ST|-1st"),
        (-2, "-02nd|-02ND|-2nd"),
        (-3, "-03rd|-03RD|-3rd"),
        (-11, "-11th|-11TH|-11th"),
        (-12, "-12th|-12TH|-12th"),
        (-13, "-13th|-13TH|-13th"),
        (-21, "-21st|-21ST|-21st"),
        (-22, "-22nd|-22ND|-22nd"),
        (-23, "-23rd|-23RD|-23rd"),
        (0, "00th|00TH|0th"),
        (1, "01st|01ST|1st"),
    ];

    for (hours, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{hours} hours', 'HH24th|HH24TH|FMHH24th')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval ordinal formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 ordinal for {hours} hours"
        );
    }
}

