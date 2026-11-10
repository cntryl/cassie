use super::parse_csv_payload;
use crate::app::CopyCsvOptions;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

fn options<'a>() -> CopyCsvOptions<'a> {
    CopyCsvOptions {
        delimiter: ',',
        quote: '"',
        escape: '"',
        null_marker: "",
        force_null: &[],
        force_not_null: &[],
    }
}

#[test]
fn should_admit_csv_quoted_null_empty_and_newline_fields() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let payload = b",\"\",\"a\n\"\"b\"\r\nx,y,z";
    // Act
    let (rows, memory) = parse_csv_payload(payload, options(), true, &controls).expect("CSV");
    // Assert
    assert_eq!(
        rows,
        vec![
            vec![None, Some(String::new()), Some("a\n\"b".into())],
            vec![Some("x".into()), Some("y".into()), Some("z".into())]
        ]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_csv_decoder_growth_before_exceeding_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let decoded = parse_csv_payload(b"a,b\n", options(), true, &controls);
    // Assert
    assert!(decoded.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_embedded_empty_unquoted_null_policy() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let options = CopyCsvOptions {
        null_marker: "\\N",
        ..options()
    };
    // Act
    let (rows, memory) = parse_csv_payload(b",\"\",\\N\n", options, true, &controls).expect("CSV");
    // Assert
    assert_eq!(rows, vec![vec![None, Some(String::new()), None]]);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_unicode_force_null_and_not_null_controls() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let options = CopyCsvOptions {
        null_marker: "ø",
        force_null: &[0],
        force_not_null: &[1],
        ..options()
    };
    // Act
    let (rows, memory) = parse_csv_payload(
        "\"ø\",ø,\"a\r\nb\"\r\n".as_bytes(),
        options,
        false,
        &controls,
    )
    .expect("CSV");
    // Assert
    assert_eq!(
        rows,
        vec![vec![None, Some("ø".into()), Some("a\r\nb".into())]]
    );
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
