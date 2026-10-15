use super::*;

#[test]
fn should_format_money_after_setting_c_monetary_locale() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET lc_monetary = 'C'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT current_setting('lc_monetary') AS locale, \
                    '12.34'::money AS plain_input, '$12.34'::money AS currency_input",
        )]),
    ];

    // Act
    let batches = run_script("issue-908-money-explicit-c-locale", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("C-locale money RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 790, 790]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("C".to_owned()),
            Some("$12.34".to_owned()),
            Some("$12.34".to_owned()),
        ]]
    );
}

#[test]
fn should_report_money_average_as_undefined_function() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT AVG('1.00'::money)")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("issue-908-money-average-resolution", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    let error = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("undefined AVG(money) ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(field, message)| {
            *field == 'M' && message.contains("function avg(money) does not exist")
        }));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

include!("../numeric/money/semantics.rs");
