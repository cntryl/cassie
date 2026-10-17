//! Simple Query preserves the entire frontend SQL message.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_message_0_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = "SELECT current_query() AS output";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-0",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![[
            0, 1, 0, 0, 0, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116, 95,
            113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116
        ]
        .to_vec()]
    );
}

#[test]
fn should_preserve_message_1_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = "  SELECT current_query() AS output ;  ";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-1",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![[
            0, 1, 0, 0, 0, 38, 32, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
            116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
            32, 59, 32, 32
        ]
        .to_vec()]
    );
}

#[test]
fn should_preserve_message_2_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = "/* α */ SELECT pg_catalog.current_query() AS output -- tail\n";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-2",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![[
            0, 1, 0, 0, 0, 61, 47, 42, 32, 206, 177, 32, 42, 47, 32, 83, 69, 76, 69, 67, 84, 32,
            112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110, 116, 95,
            113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 45,
            45, 32, 116, 97, 105, 108, 10
        ]
        .to_vec()]
    );
}

#[test]
fn should_preserve_message_3_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = "SELECT current_query() AS output; SELECT current_query() AS output";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-3",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![
            [
                0, 1, 0, 0, 0, 66, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                59, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117,
                101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116
            ]
            .to_vec(),
            [
                0, 1, 0, 0, 0, 66, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                59, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117,
                101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116
            ]
            .to_vec()
        ]
    );
}

#[test]
fn should_preserve_message_4_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = " SELECT current_query() AS output ; SELECT 7; SELECT pg_catalog.current_query() AS output; ";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-4",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![
            [
                0, 1, 0, 0, 0, 91, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116, 32, 59, 32, 83, 69, 76, 69, 67, 84, 32, 55, 59, 32, 83, 69, 76, 69, 67, 84,
                32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116, 59, 32
            ]
            .to_vec(),
            [0, 1, 0, 0, 0, 1, 55].to_vec(),
            [
                0, 1, 0, 0, 0, 91, 32, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116, 32, 59, 32, 83, 69, 76, 69, 67, 84, 32, 55, 59, 32, 83, 69, 76, 69, 67, 84,
                32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46, 99, 117, 114, 114, 101, 110,
                116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117,
                116, 59, 32
            ]
            .to_vec()
        ]
    );
}

#[test]
fn should_preserve_message_5_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = "WITH q AS (SELECT current_query() AS output) SELECT output FROM q";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-5",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![[
            0, 1, 0, 0, 0, 65, 87, 73, 84, 72, 32, 113, 32, 65, 83, 32, 40, 83, 69, 76, 69, 67, 84,
            32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83,
            32, 111, 117, 116, 112, 117, 116, 41, 32, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116,
            112, 117, 116, 32, 70, 82, 79, 77, 32, 113
        ]
        .to_vec()]
    );
}

#[test]
fn should_preserve_message_6_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = "SELECT current_query() AS output WHERE false";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-6",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        Vec::<Vec<u8>>::new()
    );
}

#[test]
fn should_preserve_message_7_given_simple_query_when_current_query_is_called() {
    // Arrange
    let sql = "SELECT lower(current_query()) AS output";
    // Act
    let batches = fixture::run_cycles(
        "query-simple-7",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![[
            0, 1, 0, 0, 0, 39, 115, 101, 108, 101, 99, 116, 32, 108, 111, 119, 101, 114, 40, 99,
            117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 97, 115, 32,
            111, 117, 116, 112, 117, 116
        ]
        .to_vec()]
    );
}
