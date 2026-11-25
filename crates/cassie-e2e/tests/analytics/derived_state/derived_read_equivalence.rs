use super::support_read_equivalence::{sql, with_fixture};

#[test]
fn should_preserve_base_rows_with_restricted_analytical_projections() {
    // Arrange
    with_fixture("analytical_row_set_proof", |cassie, session| {
        sql(cassie, session, "CREATE TABLE ap_join_filter (tenant TEXT)");
        sql(cassie, session, "INSERT INTO ap_join_filter VALUES ('b')");
        for (table, definition) in [
                (
                    "ap_filter",
                    "SELECT tenant, amount FROM ap_filter WHERE amount > 100",
                ),
                ("ap_join", "SELECT ap_join.tenant AS tenant,ap_join.amount AS amount FROM ap_join JOIN ap_join_filter ON ap_join.tenant = ap_join_filter.tenant"),
                ("ap_distinct", "SELECT DISTINCT tenant,amount FROM ap_distinct"),
                ("ap_limit", "SELECT tenant, amount FROM ap_limit LIMIT 1"),
                (
                    "ap_group",
                    "SELECT tenant, SUM(amount) AS amount FROM ap_group GROUP BY tenant",
                ),
            ] {
                sql(
                    cassie,
                    session,
                    &format!("CREATE TABLE {table} (tenant TEXT,amount INT)"),
                );
                sql(
                    cassie,
                    session,
                    &format!("INSERT INTO {table} VALUES ('a',10),('b',200),('b',200),('b',300)"),
                );
                let query = format!("SELECT tenant,amount FROM {table} ORDER BY amount");
                let baseline = sql(cassie, session, &query);
                sql(cassie, session, &format!("CREATE MATERIALIZED PROJECTION {table}_projection WITH (analytical = true) AS {definition}"));
                // Act
                let rewritten = sql(cassie, session, &query);
                // Assert
                assert_eq!(rewritten.rows(), baseline.rows(), "{definition}");
            }
    });
}

#[test]
fn should_preserve_base_values_with_rebound_analytical_projection_names() {
    // Arrange
    with_fixture("analytical_value_binding_proof", |cassie, session| {
        for (table, value) in [("ap_alias", "legacy"), ("ap_expression", "amount * 100")] {
            sql(
                cassie,
                session,
                &format!("CREATE TABLE {table} (tenant TEXT,amount INT,legacy INT)"),
            );
            sql(
                cassie,
                session,
                &format!("INSERT INTO {table} VALUES ('a',1,99),('b',2,88)"),
            );
            let queries = [
                format!("SELECT tenant,amount FROM {table} ORDER BY tenant"),
                format!("SELECT tenant,amount FROM {table} WHERE amount = 1"),
            ];
            let baseline = queries
                .iter()
                .map(|query| sql(cassie, session, query))
                .collect::<Vec<_>>();
            sql(cassie, session, &format!("CREATE MATERIALIZED PROJECTION {table}_projection WITH (analytical = true) AS SELECT tenant,{value} AS amount FROM {table}"));
            // Act
            for (query, expected) in queries.iter().zip(baseline) {
                let rewritten = sql(cassie, session, query);
                // Assert
                assert_eq!(rewritten.rows(), expected.rows(), "{query}");
            }
        }
    });
}

#[test]
fn should_preserve_base_identity_with_covering_analytical_projections() {
    // Arrange
    with_fixture("analytical_identity_proof", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE ap_identity (tenant TEXT,amount INT)",
        );
        sql(
            cassie,
            session,
            "INSERT INTO ap_identity VALUES ('a',1),('b',2)",
        );
        let queries = [
            "SELECT _id,tenant FROM ap_identity ORDER BY tenant",
            "SELECT id,tenant FROM ap_identity ORDER BY tenant",
        ];
        let baseline = queries
            .iter()
            .map(|query| sql(cassie, session, query))
            .collect::<Vec<_>>();
        sql(cassie, session, "CREATE MATERIALIZED PROJECTION ap_identity_projection WITH (analytical = true) AS SELECT tenant,amount FROM ap_identity");
        // Act
        for (query, expected) in queries.iter().zip(baseline) {
            let rewritten = sql(cassie, session, query);
            // Assert
            assert_eq!(rewritten.rows(), expected.rows(), "{query}");
        }
    });
}

#[test]
fn should_preserve_window_results_when_a_rollup_matches_aggregates() {
    // Arrange
    with_fixture("rollup_window_equivalence", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE rollup_window (tenant TEXT,event_at TEXT,amount INT)",
        );
        sql(cassie, session, "INSERT INTO rollup_window VALUES ('a','2026-01-01T12:00:00Z',1),('b','2026-01-01T12:00:00Z',2),('b','2026-01-01T12:30:00Z',3)");
        let query = "SELECT time_bucket('1 hour',event_at) AS bucket,tenant,COUNT(*) AS total,ROW_NUMBER() OVER (ORDER BY tenant) AS rn FROM rollup_window GROUP BY time_bucket('1 hour',event_at),tenant ORDER BY bucket,tenant";
        let baseline = sql(cassie, session, query);
        sql(cassie, session, "CREATE ROLLUP rollup_window_hourly ON rollup_window USING time_bucket('1 hour',event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total");
        // Act
        let rewritten = sql(cassie, session, query);
        // Assert
        assert_eq!(rewritten.rows(), baseline.rows());
    });
}
