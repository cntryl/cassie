//! PostgreSQL lexical values, descriptors and malformed-input recovery.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_integer_literal_identities_given_radix_and_digit_separators_when_queries_execute(
) {
    // Arrange
    let sql = "SELECT 0x2a, 0o52, 0b101010, 1_000_000, 0x8000_0000, \
               -0x8000_0000_0000_0000; \
               SELECT 1_2.3_4::FLOAT8, 1_0e-2::DOUBLE PRECISION";

    // Act
    let batches = run_script(
        "issue-796-radix-and-separator-wire-values",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames), None);
    let descriptions = frames
        .iter()
        .filter(|frame| frame.0 == b'T')
        .map(|frame| wire::parse_row_description(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    assert_eq!(
        descriptions[0]
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23, 23, 23, 20, 20]
    );
    assert_eq!(
        descriptions[1]
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [701, 701]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                "42",
                "42",
                "42",
                "1000000",
                "2147483648",
                "-9223372036854775808"
            ]
            .into_iter()
            .map(|value| Some(value.to_owned()))
            .collect::<Vec<_>>(),
            vec![Some("12.34".to_owned()), Some("0.1".to_owned())],
        ]
    );
}

#[test]
fn should_decode_string_literal_forms_given_unicode_escapes_and_continuations_when_values_are_projected(
) {
    // Arrange
    let sql = r#"SELECT E'it\'s\n', U&'d!0061t' UESCAPE '!', 'alpha'
               'beta', U&'\D83D\DE00', E'\xC3\xA9',
               CASE WHEN TRUE THEN E'END\'s' ELSE 'fallback' END,
               'U&"na\006de"';
               SELECT INTEGER '42', CAST('word' AS national character varying(4)),
               NULL ISNULL, 1 NOTNULL"#;

    // Act
    let batches = run_script(
        "issue-796-string-lexical-wire-values",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames), None);
    let descriptions = frames
        .iter()
        .filter(|frame| frame.0 == b'T')
        .map(|frame| wire::parse_row_description(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    assert_eq!(
        descriptions[0]
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25; 7]
    );
    assert_eq!(
        descriptions[1]
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 1043, 16, 16]
    );
    assert_eq!(descriptions[1][1].type_mod, 8);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            [
                "it's\n",
                "dat",
                "alphabeta",
                "😀",
                "é",
                "END's",
                "U&\"na\\006de\""
            ]
            .into_iter()
            .map(|value| Some(value.to_owned()))
            .collect::<Vec<_>>(),
            ["42", "word", "t", "t"]
                .into_iter()
                .map(|value| Some(value.to_owned()))
                .collect::<Vec<_>>(),
        ]
    );
}

#[test]
fn should_resolve_unicode_identifiers_given_default_and_custom_escapes_when_ddl_and_queries_share_names(
) {
    // Arrange
    let create = r#"CREATE TABLE U&"lexical_!006eames" UESCAPE '!' (U&"va\006cue" TEXT)"#;
    let insert = r#"INSERT INTO U&"lexical_\006eames" VALUES ('stored')"#;
    let select = r#"SELECT U&"va!006cue" UESCAPE '!' FROM U&"lexical_\006eames""#;

    // Act
    let batches = run_script(
        "issue-796-unicode-identifier-wire-lifecycle",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(create)]),
            Step::ThroughReady(vec![wire::simple_query_frame(insert)]),
            Step::ThroughReady(vec![wire::simple_query_frame(select)]),
        ],
    );

    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames), None);
    }
    let frames = &batches[2];
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("stored".to_owned())]]
    );
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("Unicode identifier RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns[0].name, "value");
    assert_eq!(columns[0].type_oid, 25);
}

#[test]
fn should_reject_malformed_literals_given_valid_typed_controls_when_the_connection_recovers_after_each_error(
) {
    // Arrange
    let cases = [
        ("SELECT 1__0", "42601"),
        ("SELECT 1_", "42601"),
        ("SELECT 0x__FF", "42601"),
        ("SELECT 0b2", "42601"),
        ("SELECT 0x", "42601"),
        ("SELECT 1e+_2", "42601"),
        (r"SELECT E'\uZZZZ'", "22025"),
        (r"SELECT E'\000'", "22021"),
        (r"SELECT U&'\00G0'", "42601"),
        (r"SELECT U&'\D800'", "42601"),
        (r"SELECT U&'\0000'", "42601"),
        ("SELECT 'a' 'b'", "42601"),
    ];
    let mut steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 0x2a::INT4 AS valid_control",
    )])];
    for (sql, _) in cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42::INT4 AS recovered",
        )]));
    }

    // Act
    let batches = run_script("issue-796-malformed-lexical-wire-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("42".to_owned())]]
    );
    for (index, (_, expected)) in cases.iter().enumerate() {
        let rejected = &batches[index * 2 + 1];
        assert_eq!(wire::error_code(rejected).as_deref(), Some(*expected));
        assert_eq!(wire::data_rows(rejected), Vec::<Vec<Option<String>>>::new());
        let recovered = &batches[index * 2 + 2];
        assert_eq!(wire::error_code(recovered), None);
        assert_eq!(
            wire::data_rows(recovered),
            vec![vec![Some("42".to_owned())]]
        );
        let description = recovered
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("recovery RowDescription");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    }
}
