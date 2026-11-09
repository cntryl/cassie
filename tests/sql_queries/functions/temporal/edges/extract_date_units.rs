//! PostgreSQL DATE EXTRACT field diagnostics tracked by #815.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_classify_date_extract_reserved_and_unknown_units() {
    // Arrange
    let fixture = sql_fixture("issue-815-extract-date-unit-diagnostics", &[]);
    let cases = [
        ("now", "0A000", "not supported"),
        ("today", "0A000", "not supported"),
        ("tomorrow", "0A000", "not supported"),
        ("infinity", "0A000", "not supported"),
        ("allballs", "0A000", "not supported"),
        ("january", "22023", "not recognized"),
        ("bogus", "22023", "not recognized"),
        ("ago", "22023", "not recognized"),
        ("microsecoé", "22023", "not recognized"),
        ("MiCrOsEcOnDextra", "0A000", "not supported"),
    ];

    for date in ["2000-01-01", "infinity", "-infinity", "5874897-12-31"] {
        for (unit, state, reason) in cases {
            // Act
            let result = fixture.execute(&format!("SELECT EXTRACT('{unit}' FROM DATE '{date}')"));

            // Assert
            let expected = format!(
                "unit \"{}\" {reason} for type date",
                unit.to_ascii_lowercase()
            );
            assert!(
                matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == state && error.message == expected),
                "{unit} / {date}"
            );
        }
    }

    // Act
    let null = fixture.execute("SELECT EXTRACT('bogus' FROM NULL::date) IS NULL");
    let widened = fixture.execute("SELECT DATE_PART('year', DATE '294277-01-01')");
    let supported = fixture.execute("SELECT EXTRACT('epoch' FROM DATE 'infinity')::text, EXTRACT('yrs' FROM DATE '5874897-12-31')::text");

    // Assert
    assert_eq!(null.expect("NULL date").rows(), &[vec![Value::Bool(true)]]);
    assert!(
        matches!(widened, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008")
    );
    assert_eq!(
        supported.expect("supported aliases").rows(),
        &[vec![
            Value::String("Infinity".to_owned()),
            Value::String("5874897".to_owned())
        ]]
    );
}

#[test]
fn should_clip_date_extract_unit_diagnostics_at_utf8_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-815-extract-date-unit-clipping", &[]);
    let cases = [
        ("x".repeat(78), "x".repeat(63), "22023", "not recognized"),
        ("é".repeat(40), "é".repeat(31), "22023", "not recognized"),
        (
            format!("microsecond{}", "x".repeat(70)),
            format!("microsecond{}", "x".repeat(52)),
            "0A000",
            "not supported",
        ),
    ];

    for (unit, clipped, state, reason) in cases {
        // Act
        let result = fixture.execute(&format!("SELECT EXTRACT('{unit}' FROM DATE '2000-01-01')"));

        // Assert
        let expected = format!("unit \"{clipped}\" {reason} for type date");
        assert!(
            matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == state && error.message == expected),
            "{unit}"
        );
    }
}
