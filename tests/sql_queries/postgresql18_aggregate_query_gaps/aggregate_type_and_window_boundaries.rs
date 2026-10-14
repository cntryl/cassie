//! PostgreSQL 18.6 residual aggregate and window boundaries for #806.

use cassie::app::Cassie;
use cassie::types::Value;
use std::time::Duration;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture;

type Frames = Vec<(u8, Vec<u8>)>;

fn run_invalid_percentile_and_recovery() -> Vec<Frames> {
    wire::use_local_storage();
    let path = wire::data_dir("issue-806-percentile-invalid-fraction-sqlstate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let batches = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("start Cassie before pgwire queries");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect to Cassie pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let mut batches = Vec::with_capacity(2);
        for sql in [
            "SELECT percentile_cont(-0.1) WITHIN GROUP (ORDER BY value::FLOAT8) FROM (VALUES (1)) AS input(value)",
            "SELECT 42",
        ] {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
            batches.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    });

    let _ = std::fs::remove_dir_all(path);
    batches
}

#[test]
fn should_preserve_bitwise_aggregate_type_identity_across_empty_null_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-bitwise-aggregate-input-type-identity",
        &[
            "CREATE TABLE issue_806_bitwise_type_values (small_value SMALLINT, int_value INTEGER, big_value BIGINT)",
            "INSERT INTO issue_806_bitwise_type_values VALUES (-2,-2,-2),(7,7,7),(NULL,NULL,NULL)",
        ],
    );
    let values_query = "SELECT bit_and(small_value), bit_or(small_value), bit_xor(small_value), bit_and(int_value), bit_or(int_value), bit_xor(int_value), bit_and(big_value), bit_or(big_value), bit_xor(big_value) FROM issue_806_bitwise_type_values";
    let all_null_query = format!("{values_query} WHERE small_value IS NULL");
    let empty_query = format!("{values_query} WHERE FALSE");

    // Act
    let values = fixture.execute(values_query);
    let all_null = fixture.execute(&all_null_query);
    let empty = fixture.execute(&empty_query);

    // Assert
    let expected_oids = vec![21, 21, 21, 23, 23, 23, 20, 20, 20];
    let values = values.expect("bitwise aggregates retain each integer input width");
    assert_eq!(
        values.rows(),
        &[vec![
            Value::Int64(6),
            Value::Int64(-1),
            Value::Int64(-7),
            Value::Int64(6),
            Value::Int64(-1),
            Value::Int64(-7),
            Value::Int64(6),
            Value::Int64(-1),
            Value::Int64(-7),
        ]]
    );
    assert_eq!(
        values
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );

    let expected_nulls = &[vec![Value::Null; 9]];
    for result in [all_null, empty] {
        let result = result.expect("bitwise aggregates return NULL without non-NULL inputs");
        assert_eq!(result.rows(), expected_nulls);
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            expected_oids
        );
    }
}

#[test]
fn should_preserve_ordered_set_percentile_array_results_with_exact_oids() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-ordered-set-array-result-oids",
        &[
            "CREATE TABLE issue_806_percentile_array_types (value SMALLINT)",
            "INSERT INTO issue_806_percentile_array_types VALUES (1),(2),(3),(4)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT percentile_cont(ARRAY[0.25,0.5,1.0]) WITHIN GROUP (ORDER BY value::FLOAT8), percentile_disc(ARRAY[0.25,0.5,1.0]) WITHIN GROUP (ORDER BY value) FROM issue_806_percentile_array_types",
    );

    // Assert
    let result = result.expect("ordered-set percentile arrays preserve their output identities");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Json(serde_json::json!([1.75, 2.5, 4.0])),
            Value::Json(serde_json::json!([1, 2, 4])),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1022, 1005]
    );
}

#[test]
fn should_define_regression_statistics_for_zero_or_one_pairs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-regression-zero-one-pair-results",
        &[
            "CREATE TABLE issue_806_regression_edge_values (x FLOAT8, y FLOAT8)",
            "INSERT INTO issue_806_regression_edge_values VALUES (2,4),(NULL,3),(2,NULL)",
        ],
    );
    let query = "SELECT corr(y,x), covar_pop(y,x), covar_samp(y,x), regr_count(y,x), regr_slope(y,x), regr_intercept(y,x), regr_r2(y,x), regr_sxx(y,x), regr_sxy(y,x), regr_syy(y,x), regr_avgx(y,x), regr_avgy(y,x) FROM issue_806_regression_edge_values";

    // Act
    let empty = fixture.execute(&format!("{query} WHERE FALSE"));
    let one_pair = fixture.execute(&format!("{query} WHERE x = 2 AND y = 4"));

    // Assert
    let expected_oids = vec![701, 701, 701, 20, 701, 701, 701, 701, 701, 701, 701, 701];
    let empty = empty.expect("regression aggregates over an empty input");
    assert_eq!(
        empty.rows(),
        &[vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int64(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
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

    let one_pair = one_pair.expect("regression aggregates over one non-NULL pair");
    assert_eq!(
        one_pair.rows(),
        &[vec![
            Value::Null,
            Value::Float64(0.0),
            Value::Null,
            Value::Int64(1),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Float64(0.0),
            Value::Float64(0.0),
            Value::Float64(0.0),
            Value::Float64(2.0),
            Value::Float64(4.0),
        ]]
    );
    assert_eq!(
        one_pair
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );
}

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
fn should_keep_empty_frames_visible_at_window_partition_boundaries() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-frame-empty-boundaries",
        &[
            "CREATE TABLE issue_806_window_boundary_values (bucket TEXT, id INT, amount INT)",
            "INSERT INTO issue_806_window_boundary_values VALUES ('a',1,10),('a',2,20),('a',3,30),('b',1,5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, id, amount, SUM(amount) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING), COUNT(*) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING), SUM(amount) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING), COUNT(*) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING) FROM issue_806_window_boundary_values ORDER BY bucket,id",
    );

    // Assert
    let result = result.expect("window frames preserve empty boundary results");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(1),
                Value::Int64(10),
                Value::Null,
                Value::Int64(0),
                Value::Int64(20),
                Value::Int64(1),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(20),
                Value::Int64(10),
                Value::Int64(1),
                Value::Int64(30),
                Value::Int64(1),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(3),
                Value::Int64(30),
                Value::Int64(20),
                Value::Int64(1),
                Value::Null,
                Value::Int64(0),
            ],
            vec![
                Value::String("b".to_owned()),
                Value::Int64(1),
                Value::Int64(5),
                Value::Null,
                Value::Int64(0),
                Value::Null,
                Value::Int64(0),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 23, 20, 20, 20, 20]
    );
}

#[test]
fn should_report_invalid_ordered_set_percentile_fraction_sqlstate_over_pgwire() {
    // Arrange
    let batches = run_invalid_percentile_and_recovery();

    // Act
    let error = batches.first().expect("invalid percentile response");
    let recovery = batches.get(1).expect("post-error recovery response");
    let message = error
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, body)| {
            wire::parse_error_fields(body)
                .into_iter()
                .find_map(|(key, value)| (key == 'M').then_some(value))
        });

    // Assert
    assert_eq!(wire::error_code(error).as_deref(), Some("22003"));
    assert_eq!(
        message.as_deref(),
        Some("percentile value -0.1 is not between 0 and 1")
    );
    assert!(
        error
            .iter()
            .all(|(tag, _)| !matches!(*tag, b'T' | b'D' | b'C')),
        "an invalid percentile must not publish result frames: {error:?}"
    );
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("42".to_owned())]]);
}

#[test]
fn should_preserve_mode_result_identity_for_non_int4_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-mode-result-type-identity",
        &[
            "CREATE TABLE issue_806_mode_type_values (small_value SMALLINT, big_value BIGINT, text_value TEXT)",
            "INSERT INTO issue_806_mode_type_values VALUES (-1,10000000000,'z'),(-1,10000000000,'z'),(2,20000000000,'a')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT mode() WITHIN GROUP (ORDER BY small_value), mode() WITHIN GROUP (ORDER BY big_value), mode() WITHIN GROUP (ORDER BY text_value) FROM issue_806_mode_type_values",
    );

    // Assert
    let result = result.expect("mode returns the ordering expression's result type");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(-1),
            Value::Int64(10_000_000_000),
            Value::String("z".to_owned()),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![21, 20, 25]
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

#[test]
fn should_define_statistical_aggregate_results_for_nonfinite_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-806-nonfinite-statistical-inputs", &[]);
    let query = "SELECT corr(x,y), covar_pop(x,y), covar_samp(x,y), regr_count(y,x), regr_slope(y,x), regr_intercept(y,x), regr_r2(y,x), regr_sxx(y,x), regr_sxy(y,x), regr_syy(y,x), regr_avgx(y,x), regr_avgy(y,x) FROM (VALUES ({x}::FLOAT8,1::FLOAT8),(1,2),(2,3)) AS pairs(x,y)";
    let nan = fixture.execute(&query.replace("{x}", "'NaN'"));
    let infinity = fixture.execute(&query.replace("{x}", "'Infinity'"));

    // Act
    let nan = nan.expect("statistical aggregates over NaN input");
    let infinity = infinity.expect("statistical aggregates over positive infinity input");

    // Assert
    let expected_oids = vec![701, 701, 701, 20, 701, 701, 701, 701, 701, 701, 701, 701];
    for result in [&nan, &infinity] {
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            expected_oids
        );
        assert_eq!(result.rows()[0][3], Value::Int64(3));
        assert_eq!(result.rows()[0][9], Value::Float64(2.0));
        assert_eq!(result.rows()[0][11], Value::Float64(2.0));
        for index in [0, 1, 2, 4, 5, 6, 7, 8] {
            assert!(
                matches!(&result.rows()[0][index], Value::Float64(value) if value.is_nan()),
                "statistical output at index {index} should be NaN: {:?}",
                result.rows()[0][index]
            );
        }
    }
    assert!(matches!(
        &nan.rows()[0][10],
        Value::Float64(value) if value.is_nan()
    ));
    assert!(matches!(
        &infinity.rows()[0][10],
        Value::Float64(value) if value.is_infinite() && value.is_sign_positive()
    ));
}
