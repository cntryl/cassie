use super::*;

#[test]
fn should_apply_quote_policies_to_json_query_scalar_strings() {
    // Arrange
    let sql = "SELECT JSON_QUERY(\
                   '{\"name\":\"Ada\",\"items\":[1,2]}'::JSONB, \
                   '$.name' RETURNING TEXT KEEP QUOTES ON SCALAR STRING), \
                 JSON_QUERY(\
                   '{\"name\":\"Ada\",\"items\":[1,2]}'::JSONB, \
                   '$.name' RETURNING TEXT OMIT QUOTES ON SCALAR STRING), \
                 JSON_QUERY(\
                   '{\"name\":\"Ada\",\"items\":[1,2]}'::JSONB, \
                   '$.items' RETURNING TEXT OMIT QUOTES ON SCALAR STRING), \
                 pg_typeof(JSON_QUERY(\
                   '{\"name\":\"Ada\",\"items\":[1,2]}'::JSONB, \
                   '$.name' RETURNING TEXT OMIT QUOTES ON SCALAR STRING))::OID";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-json-query-scalar-quote-policies",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_QUERY quote-policy RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 25, 26]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("\"Ada\"".to_owned()),
            Some("Ada".to_owned()),
            Some("[1, 2]".to_owned()),
            Some("25".to_owned()),
        ]]
    );
}
