use super::support_read_equivalence::{sql, with_fixture};
use cassie::executor::ColumnMeta;
use cassie::types::Value;

fn expected_columns() -> Vec<ColumnMeta> {
    vec![
        ColumnMeta {
            name: "n".into(),
            data_type: "bigint".into(),
            type_oid: 20,
            typlen: 8,
            atttypmod: -1,
            format_code: 0,
            nullable: true,
        },
        ColumnMeta {
            name: "note".into(),
            data_type: "text".into(),
            type_oid: 25,
            typlen: -1,
            atttypmod: -1,
            format_code: 0,
            nullable: true,
        },
    ]
}

#[test]
fn should_report_declared_types_for_direct_materialized_projection_reads() {
    // Arrange
    with_fixture("direct_projection_type_metadata", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE projection_type_source (n BIGINT,note TEXT)",
        );
        sql(
            cassie,
            session,
            "INSERT INTO projection_type_source VALUES (3,NULL),(1,'first'),(2,'second')",
        );
        sql(
            cassie,
            session,
            "CREATE MATERIALIZED PROJECTION projection_type_offset WITH (analytical = true) AS SELECT n,note FROM projection_type_source ORDER BY n OFFSET 1",
        );
        sql(
            cassie,
            session,
            "CREATE MATERIALIZED PROJECTION projection_type_all WITH (analytical = true) AS SELECT n,note FROM projection_type_source ORDER BY n",
        );

        // Act
        let offset = sql(
            cassie,
            session,
            "SELECT n,note FROM projection_type_offset ORDER BY n",
        );
        let unrestricted = sql(
            cassie,
            session,
            "SELECT n,note FROM projection_type_all ORDER BY n",
        );
        let offset_description = cassie
            .describe_sql("SELECT n,note FROM projection_type_offset ORDER BY n")
            .expect("describe direct offset projection");
        let unrestricted_description = cassie
            .describe_sql("SELECT n,note FROM projection_type_all ORDER BY n")
            .expect("describe direct unrestricted projection");

        // Assert
        assert_eq!(offset_description, expected_columns());
        assert_eq!(unrestricted_description, expected_columns());
        assert_eq!(offset.columns, expected_columns());
        assert_eq!(
            offset.rows,
            vec![
                vec![Value::Int64(2), Value::String("second".into())],
                vec![Value::Int64(3), Value::Null],
            ]
        );
        assert_eq!(unrestricted.columns, expected_columns());
        assert_eq!(
            unrestricted.rows,
            vec![
                vec![Value::Int64(1), Value::String("first".into())],
                vec![Value::Int64(2), Value::String("second".into())],
                vec![Value::Int64(3), Value::Null],
            ]
        );
    });
}
