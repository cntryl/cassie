//! PostgreSQL array identities and encodings for ordered-set percentile output.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

fn postgres_array(element_oid: i32, elements: &[Option<&[u8]>]) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(&1_i32.to_be_bytes());
    value.extend_from_slice(&i32::from(elements.iter().any(Option::is_none)).to_be_bytes());
    value.extend_from_slice(&element_oid.to_be_bytes());
    value.extend_from_slice(
        &i32::try_from(elements.len())
            .expect("array element count")
            .to_be_bytes(),
    );
    value.extend_from_slice(&1_i32.to_be_bytes());
    for element in elements {
        if let Some(element) = element {
            value.extend_from_slice(
                &i32::try_from(element.len())
                    .expect("array element size")
                    .to_be_bytes(),
            );
            value.extend_from_slice(element);
        } else {
            value.extend_from_slice(&(-1_i32).to_be_bytes());
        }
    }
    value
}

fn data_row(fields: &[&[u8]]) -> Vec<u8> {
    let mut row = u16::try_from(fields.len())
        .expect("result field count")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        row.extend_from_slice(
            &i32::try_from(field.len())
                .expect("result field size")
                .to_be_bytes(),
        );
        row.extend_from_slice(field);
    }
    row
}

fn interval_binary(micros: i64) -> [u8; 16] {
    let mut value = [0_u8; 16];
    value[..8].copy_from_slice(&micros.to_be_bytes());
    value
}

#[test]
fn should_encode_percentile_array_results_given_postgresql_result_formats() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_806_pgwire_percentile_values (value SMALLINT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_806_pgwire_percentile_values VALUES (1),(2),(3),(4)",
            Vec::new(),
        ),
    ];
    let sql = "SELECT percentile_cont(ARRAY[0.25,0.5,1.0]) WITHIN GROUP (ORDER BY value::FLOAT8), \
                      percentile_disc(ARRAY[0.25,0.5,1.0]) WITHIN GROUP (ORDER BY value), \
                      percentile_disc(ARRAY[0.0,NULL,1.0]) WITHIN GROUP (ORDER BY value) \
               FROM issue_806_pgwire_percentile_values";
    let cycles = [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true));

    // Act
    let batches = fixture::run_cycles("issue-806-pgwire-percentile-arrays", &setup, cycles.into());

    // Assert
    let text = &batches[0];
    fixture::assert_success(text);
    let text_description = text
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text RowDescription");
    assert_eq!(
        wire::parse_row_description(&text_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1022, 1005, 1005]
    );
    assert_eq!(
        wire::data_rows(text),
        vec![vec![
            Some("{1.75,2.5,4}".into()),
            Some("{1,2,4}".into()),
            Some("{1,NULL,4}".into()),
        ]]
    );

    let binary = &batches[1];
    fixture::assert_success(binary);
    let binary_description = binary
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .next_back()
        .expect("binary portal RowDescription");
    assert_eq!(
        binary_description
            .iter()
            .map(|column| (column.type_oid, column.format_code))
            .collect::<Vec<_>>(),
        vec![(1022, 1), (1005, 1), (1005, 1)]
    );
    let float8_array = postgres_array(
        701,
        &[
            Some(&1.75_f64.to_be_bytes()),
            Some(&2.5_f64.to_be_bytes()),
            Some(&4.0_f64.to_be_bytes()),
        ],
    );
    let int2_array = postgres_array(21, &[Some(&[0, 1]), Some(&[0, 2]), Some(&[0, 4])]);
    let nullable_int2_array = postgres_array(21, &[Some(&[0, 1]), None, Some(&[0, 4])]);
    let expected = data_row(&[&float8_array, &int2_array, &nullable_int2_array]);
    assert_eq!(fixture::data_row_payloads(binary), vec![expected]);
}

#[test]
fn should_preserve_percentile_array_identity_through_a_cte_projection() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_806_pgwire_percentile_cte_values (value SMALLINT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_806_pgwire_percentile_cte_values VALUES (1),(2),(3),(4)",
            Vec::new(),
        ),
    ];
    let sql = "WITH percentile_result AS ( \
                  SELECT percentile_cont(ARRAY[0.25,0.75]) WITHIN GROUP (ORDER BY value::FLOAT8) AS selected \
                  FROM issue_806_pgwire_percentile_cte_values \
               ) SELECT selected, pg_typeof(selected)::OID FROM percentile_result";
    let cycles = [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true));

    // Act
    let batches = fixture::run_cycles(
        "issue-806-pgwire-percentile-cte-array",
        &setup,
        cycles.into(),
    );

    // Assert
    for batch in &batches {
        fixture::assert_success(batch);
        let description = batch
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, payload)| wire::parse_row_description(payload))
            .next_back()
            .expect("CTE RowDescription");
        assert_eq!(
            description
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![1022, 26]
        );
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{1.75,3.25}".into()), Some("1022".into())]]
    );
    let array = postgres_array(
        701,
        &[Some(&1.75_f64.to_be_bytes()), Some(&3.25_f64.to_be_bytes())],
    );
    let oid = 1_022_u32.to_be_bytes();
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![data_row(&[&array, &oid])]
    );
}

#[test]
fn should_encode_interval_percentile_array_results_given_postgresql_result_formats() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_806_pgwire_interval_percentile_values (duration INTERVAL)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_806_pgwire_interval_percentile_values VALUES \
             (INTERVAL '0 hours'), (INTERVAL '4 hours'), (INTERVAL '8 hours'), \
             (INTERVAL '12 hours'), (NULL)",
            Vec::new(),
        ),
    ];
    let sql = "SELECT percentile_cont(ARRAY[0.25,0.5,0.75]) \
                      WITHIN GROUP (ORDER BY duration) \
               FROM issue_806_pgwire_interval_percentile_values";
    let cycles = [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true));

    // Act
    let batches = fixture::run_cycles(
        "issue-806-pgwire-interval-percentile-arrays",
        &setup,
        cycles.into(),
    );

    // Assert
    let text = &batches[0];
    fixture::assert_success(text);
    let text_description = text
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text RowDescription");
    assert_eq!(
        wire::parse_row_description(&text_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1187]
    );
    assert_eq!(
        wire::data_rows(text),
        vec![vec![Some("{03:00:00,06:00:00,09:00:00}".into())]]
    );

    let binary = &batches[1];
    fixture::assert_success(binary);
    let binary_description = binary
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .next_back()
        .expect("binary portal RowDescription");
    assert_eq!(
        binary_description
            .iter()
            .map(|column| (column.type_oid, column.format_code))
            .collect::<Vec<_>>(),
        vec![(1187, 1)]
    );
    let three_hours = interval_binary(10_800_000_000);
    let six_hours = interval_binary(21_600_000_000);
    let nine_hours = interval_binary(32_400_000_000);
    let interval_array = postgres_array(
        1186,
        &[Some(&three_hours), Some(&six_hours), Some(&nine_hours)],
    );
    let expected = data_row(&[&interval_array]);
    assert_eq!(fixture::data_row_payloads(binary), vec![expected]);
}
