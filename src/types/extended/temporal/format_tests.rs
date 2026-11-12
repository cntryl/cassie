use super::checked::CHECK_INTERVAL;
use super::{literal_is_whitespace_with_check, parse, push_str_with_check, to_char, Moment};
use crate::types::extended::temporal::calendar::Fields;

fn moment(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
    micros: i64,
) -> Moment {
    Moment {
        fields: Fields {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        },
        zone_east: None,
        is_interval: false,
    }
}

#[test]
fn should_render_postgresql_templates() {
    // Arrange
    let value = moment(2002, 4, 20, 17, 31, 12, 660_123);

    // Act
    let twelve = to_char(&value, "HH12:MI:SS", "");
    let filled = to_char(&value, "FMMonth DD, YYYY", "");
    let micros = to_char(&value, "YYYY-MM-DD\"T\"HH24:MI:SS.US", "");
    let iso = to_char(&moment(2006, 1, 1, 0, 0, 0, 0), "IYYY-IW-ID", "");

    // Assert
    assert_eq!(twelve, "05:31:12");
    assert_eq!(filled, "April 20, 2002");
    assert_eq!(micros, "2002-04-20T17:31:12.660123");
    assert_eq!(iso, "2005-52-7");
}

#[test]
fn should_parse_formatted_input() {
    // Arrange
    let inputs = [
        ("05 Dec 2000", "DD Mon YYYY"),
        ("05 dec 2000", "DD MON YYYY"),
        ("2006-42-4", "IYYY-IW-ID"),
        ("05 Dec 2000 15:12:02.020001", "DD Mon YYYY HH24:MI:SS.US"),
    ];

    // Act
    let parsed = inputs.map(|(input, template)| parse(input, template).map(|parsed| parsed.fields));

    // Assert
    let first = parsed[0].as_ref().expect("date");
    assert_eq!((first.year, first.month, first.day), (2000, 12, 5));
    assert_eq!(parsed[1].as_ref().map(|fields| fields.month).ok(), Some(12));
    let iso = parsed[2].as_ref().expect("iso");
    assert_eq!((iso.year, iso.month, iso.day), (2006, 10, 19));
    assert_eq!(
        parsed[3].as_ref().map(|fields| fields.micros).ok(),
        Some(20_001)
    );
    assert_eq!(
        parse("not-a-date", "YYYY-MM-DD")
            .map_err(|error| error.sqlstate)
            .err(),
        Some("22007")
    );
}

#[test]
fn should_check_cancellation_while_scanning_a_long_whitespace_literal() {
    // Arrange
    let literal = " ".repeat(10_000);
    let mut checkpoints = 0;

    // Act
    let result = literal_is_whitespace_with_check(&literal, &mut || {
        checkpoints += 1;
        if checkpoints == 2 {
            Err("canceled")
        } else {
            Ok(())
        }
    });

    // Assert
    assert_eq!(result, Err("canceled"));
    assert_eq!(checkpoints, 2);
}

#[test]
fn should_check_cancellation_while_rendering_a_long_literal() {
    // Arrange
    let literal = "é".repeat(10_000);
    let mut output = String::new();
    let mut checkpoints = 0;

    // Act
    let result = push_str_with_check(&mut output, &literal, &mut || {
        checkpoints += 1;
        if checkpoints == 2 {
            Err("canceled")
        } else {
            Ok(())
        }
    });

    // Assert
    assert_eq!(result, Err("canceled"));
    assert_eq!(output.chars().count(), 2 * CHECK_INTERVAL);
    assert_eq!(checkpoints, 2);
}
