//! Finite DERIVED-01 qualification for a positive-OFFSET projection definition.

use super::support_read_equivalence::{sql, with_fixture};
use cassie::app::Cassie;
use cassie::executor::ColumnMeta;
use cassie::types::Value;

const BASE_QUERY: &str = "SELECT n,note FROM ap_offset_source ORDER BY n";

fn optimized_count(cassie: &Cassie) -> u64 {
    cassie.metrics()["projections"]["mixed_execution_optimized"]
        .as_u64()
        .expect("completed analytical substitution count")
}

fn assert_retired(cassie: &Cassie) {
    let metrics = cassie.metrics();
    for (section, field) in [
        ("query", "current_accounted_memory_bytes"),
        ("runtime", "running_queries"),
        ("runtime", "active_operator_workers"),
    ] {
        assert_eq!(
            metrics[section][field]
                .as_u64()
                .expect("public query cleanup counter"),
            0,
            "{section}.{field}: {metrics}"
        );
    }
}

#[test]
fn should_decline_offset_projection_substitution_while_using_unrestricted_twin() {
    for (label, suffix, optimized_delta) in [
        ("analytical_positive_offset", " OFFSET 1", 0),
        ("analytical_unrestricted_twin", "", 1),
    ] {
        // Arrange: separate fixtures prevent an eligible sibling masking rejection.
        with_fixture(label, |cassie, session| {
            sql(
                cassie,
                session,
                "CREATE TABLE ap_offset_source (n BIGINT,note TEXT)",
            );
            sql(
                cassie,
                session,
                "INSERT INTO ap_offset_source VALUES (3,NULL),(1,'first'),(2,'second')",
            );
            let expected = vec![
                vec![Value::Int64(1), Value::String("first".into())],
                vec![Value::Int64(2), Value::String("second".into())],
                vec![Value::Int64(3), Value::Null],
            ];
            let baseline = sql(cassie, session, BASE_QUERY);
            assert_eq!(baseline.rows, expected);
            assert_eq!(
                baseline.columns,
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
            );
            assert_retired(cassie);
            sql(
                cassie,
                session,
                &format!(
                    "CREATE MATERIALIZED PROJECTION ap_offset WITH (analytical = true) \
                     AS SELECT n,note FROM ap_offset_source ORDER BY n{suffix}"
                ),
            );

            // Act: isolate the completed-path counter around only the base query.
            let before = optimized_count(cassie);
            let base = sql(cassie, session, BASE_QUERY);
            let after = optimized_count(cassie);
            assert_retired(cassie);
            let direct = sql(cassie, session, "SELECT n,note FROM ap_offset ORDER BY n");
            assert_retired(cassie);

            // Assert: the restricted candidate omits row one, but the base never does.
            assert_eq!(base.rows, baseline.rows, "{label}");
            assert_eq!(base.columns, baseline.columns, "{label}");
            assert_eq!(after, before + optimized_delta, "{label}");
            assert_eq!(direct.columns, baseline.columns, "{label}");
            assert_eq!(
                direct.rows,
                if suffix.is_empty() {
                    expected
                } else {
                    expected[1..].to_vec()
                },
                "{label}"
            );
        });
    }
}
