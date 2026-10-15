#[test]
fn should_rank_hypothetical_values_with_empty_or_null_orderings() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-hypothetical-empty-null-inputs",
        &[
            "CREATE TABLE issue_806_hypothetical_edge_values (score INT)",
            "INSERT INTO issue_806_hypothetical_edge_values VALUES (NULL),(NULL)",
        ],
    );
    let query = "SELECT rank(2) WITHIN GROUP (ORDER BY score), dense_rank(2) WITHIN GROUP (ORDER BY score), percent_rank(2) WITHIN GROUP (ORDER BY score), cume_dist(2) WITHIN GROUP (ORDER BY score) FROM issue_806_hypothetical_edge_values";

    // Act
    let all_null = fixture.execute(query);
    let empty = fixture.execute(&format!("{query} WHERE FALSE"));

    // Assert
    let expected_oids = vec![20, 20, 701, 701];
    let all_null = all_null.expect("hypothetical rank functions over NULL sort values");
    assert_eq!(
        all_null.rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(1),
            Value::Float64(0.0),
            Value::Float64(1.0 / 3.0),
        ]]
    );
    assert_eq!(
        all_null
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );

    let empty = empty.expect("hypothetical rank functions over an empty input");
    assert_eq!(
        empty.rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(1),
            Value::Float64(0.0),
            Value::Float64(1.0),
        ]]
    );
    assert_eq!(
        empty
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );
}

#[test]
fn should_rank_multi_column_hypotheticals_at_empty_null_boundaries() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-multicolumn-hypothetical-empty-null-direct-arguments",
        &[
            "CREATE TABLE issue_806_hypothetical_multi_values (number_value INT, text_value TEXT)",
            "INSERT INTO issue_806_hypothetical_multi_values VALUES (1,'a'),(NULL,'z'),(2,'b')",
        ],
    );
    let aggregates = "rank({direct}) WITHIN GROUP (ORDER BY number_value,text_value), dense_rank({direct}) WITHIN GROUP (ORDER BY number_value,text_value), percent_rank({direct}) WITHIN GROUP (ORDER BY number_value,text_value), cume_dist({direct}) WITHIN GROUP (ORDER BY number_value,text_value)";
    let empty_query = format!(
        "SELECT {}, {} FROM issue_806_hypothetical_multi_values WHERE FALSE",
        aggregates.replace("{direct}", "2,'b'"),
        aggregates.replace("{direct}", "NULL::INT,NULL::TEXT")
    );
    let null_direct_query = format!(
        "SELECT {} FROM issue_806_hypothetical_multi_values",
        aggregates.replace("{direct}", "NULL::INT,NULL::TEXT")
    );

    // Act
    let empty = fixture.execute(&empty_query);
    let null_direct = fixture.execute(&null_direct_query);

    // Assert
    let expected_oids = vec![20, 20, 701, 701, 20, 20, 701, 701];
    let empty = empty.expect("multi-column hypothetical aggregates over no rows");
    assert_eq!(
        empty.rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(1),
            Value::Float64(0.0),
            Value::Float64(1.0),
            Value::Int64(1),
            Value::Int64(1),
            Value::Float64(0.0),
            Value::Float64(1.0),
        ]]
    );
    assert_eq!(
        empty
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );

    let null_direct = null_direct.expect("NULL direct arguments sort after non-NULL tuples");
    assert_eq!(
        null_direct.rows(),
        &[vec![
            Value::Int64(4),
            Value::Int64(4),
            Value::Float64(1.0),
            Value::Float64(1.0),
        ]]
    );
    assert_eq!(
        null_direct
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 20, 701, 701]
    );
}
