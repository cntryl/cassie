//! FLOAT precision selection through Parse, Describe, Bind, and Execute.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_bind_float_precision_casts_using_native_type_metadata() {
    // Arrange
    let statement = "float_precision_casts";
    let portal = "float_precision_portal";
    let null_portal = "float_precision_null_portal";
    let sql = "SELECT $1::FLOAT(24) AS real_value, $2::FLOAT(25) AS float_value";
    let cycles = vec![
        vec![
            wire::parse_frame_with_types(statement, sql, &[0, 0]),
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ],
        vec![
            wire::bind_frame(portal, statement, &["1.23456789", "1.23456789"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ],
        vec![
            wire::bind_frame_with_formats(null_portal, statement, &[], &[None, None], &[]),
            wire::describe_portal_frame(null_portal),
            wire::execute_frame(null_portal),
            wire::sync_frame(),
        ],
        vec![wire::simple_query_frame(
            "SELECT CAST('1.5' AS FLOAT(0)) AS value",
        )],
        vec![wire::simple_query_frame(
            "SELECT CAST('1.5' AS FLOAT(54)) AS value",
        )],
        vec![wire::simple_query_frame(
            "SELECT CAST('1.5' AS FLOAT(+24)) AS value",
        )],
    ];

    // Act
    let batches = fixture::run_cycles("issue-796-float-precision-parameters", &[], cycles);
    let described = &batches[0];
    let executed = &batches[1];
    let nulls = &batches[2];

    // Assert
    fixture::assert_success(described);
    fixture::assert_success(executed);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("inferred FLOAT precision ParameterDescription")
                .1
        ),
        vec![700, 701]
    );
    let expected_columns = vec![
        ("real_value", 700, 4_i16, -1, 0_i16),
        ("float_value", 701, 8_i16, -1, 0_i16),
    ];
    for frames in [described, executed, nulls] {
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("FLOAT precision RowDescription");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| (
                    column.name.as_str(),
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                ))
                .collect::<Vec<_>>(),
            expected_columns
        );
    }
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![
            Some("1.2345679".to_owned()),
            Some("1.23456789".to_owned()),
        ]]
    );
    fixture::assert_success(nulls);
    assert_eq!(wire::error_code(&batches[3]).as_deref(), Some("22023"));
    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("22023"));
    assert_eq!(wire::error_code(&batches[5]).as_deref(), Some("42601"));
    assert_eq!(wire::data_rows(nulls), vec![vec![None, None]]);
}
