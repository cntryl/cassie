//! Repeatable committed Data composes with each command's own staged changes.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_keep_later_modifying_ctes_on_the_precommand_view() {
    // Arrange
    for isolation in ["READ COMMITTED", "REPEATABLE READ READ WRITE"] {
        let fixture = sql_fixture(
            "rr-later-cte-view",
            &[
                "CREATE TABLE rr_cte_source (id INT PRIMARY KEY)",
                "CREATE TABLE rr_cte_target (id INT PRIMARY KEY)",
                "INSERT INTO rr_cte_source VALUES (1)",
            ],
        );
        fixture
            .execute(&format!("BEGIN ISOLATION LEVEL {isolation}"))
            .expect("begin");
        fixture
            .execute("INSERT INTO rr_cte_source VALUES (2)")
            .expect("precommand own row");

        // Act
        let observed = fixture.execute("WITH first_change AS (INSERT INTO rr_cte_source VALUES (3) RETURNING id), second_change AS (INSERT INTO rr_cte_target SELECT id FROM rr_cte_source RETURNING id) SELECT id FROM second_change ORDER BY id").expect("later modifying CTE");
        let own_rows = fixture
            .execute("SELECT id FROM rr_cte_source ORDER BY id")
            .expect("own effects after command");
        fixture.execute("ROLLBACK").expect("retire transaction");

        // Assert
        assert_eq!(
            observed.rows(),
            &[vec![Value::Int64(1)], vec![Value::Int64(2)]],
            "{isolation}"
        );
        assert_eq!(
            own_rows.rows(),
            &[
                vec![Value::Int64(1)],
                vec![Value::Int64(2)],
                vec![Value::Int64(3)]
            ],
            "{isolation}"
        );
    }
}

#[test]
fn should_keep_foreign_key_checks_on_live_own_changes() {
    // Arrange
    for isolation in ["READ COMMITTED", "REPEATABLE READ READ WRITE"] {
        let fixture = sql_fixture("rr-cte-own-parent", &[
            "CREATE TABLE rr_cte_parent (id INT PRIMARY KEY)",
            "CREATE TABLE rr_cte_child (id INT PRIMARY KEY, parent_id INT REFERENCES rr_cte_parent(id))",
        ]);
        fixture
            .execute(&format!("BEGIN ISOLATION LEVEL {isolation}"))
            .expect("begin");

        // Act
        let observed = fixture.execute("WITH parent_change AS (INSERT INTO rr_cte_parent VALUES (1) RETURNING id), child_change AS (INSERT INTO rr_cte_child VALUES (2, 1) RETURNING id) SELECT id FROM child_change").expect("own parent satisfies constraint");
        fixture.execute("ROLLBACK").expect("retire transaction");

        // Assert
        assert_eq!(observed.rows(), &[vec![Value::Int64(2)]], "{isolation}");
    }
}

#[test]
fn should_preserve_precommand_sources_for_sibling_dml_read_boundaries() {
    // Arrange
    let cases = [
        ("INSERT INTO rr_sibling_target (id, value) SELECT id, 7 FROM rr_sibling_source ON CONFLICT (id) DO UPDATE SET value = excluded.value RETURNING id", vec![1_i64, 2]),
        ("UPDATE rr_sibling_target SET value = (SELECT count(*) FROM rr_sibling_source) WHERE id = 1 RETURNING value", vec![2]),
        ("DELETE FROM rr_sibling_target WHERE id <= (SELECT count(*) FROM rr_sibling_source) RETURNING id", vec![1, 2]),
    ];
    for isolation in ["READ COMMITTED", "REPEATABLE READ READ WRITE"] {
        for (second_change, expected) in &cases {
            let fixture = sql_fixture(
                "rr-sibling-dml-source",
                &[
                    "CREATE TABLE rr_sibling_source (id INT PRIMARY KEY)",
                    "CREATE TABLE rr_sibling_target (id INT PRIMARY KEY, value BIGINT)",
                    "INSERT INTO rr_sibling_source VALUES (1)",
                    "INSERT INTO rr_sibling_target VALUES (1, 0), (2, 0), (3, 0)",
                ],
            );
            fixture
                .execute(&format!("BEGIN ISOLATION LEVEL {isolation}"))
                .expect("begin");
            fixture
                .execute("INSERT INTO rr_sibling_source VALUES (2)")
                .expect("precommand own row");

            // Act
            let observed = fixture.execute(&format!("WITH first_change AS (INSERT INTO rr_sibling_source VALUES (3) RETURNING id), second_change AS ({second_change}) SELECT * FROM second_change")).expect("sibling DML source");
            let mut values = observed
                .rows()
                .iter()
                .map(|row| match row.as_slice() {
                    [Value::Int64(value)] => *value,
                    _ => panic!("one integer result: {row:?}"),
                })
                .collect::<Vec<_>>();
            values.sort_unstable();
            fixture.execute("ROLLBACK").expect("retire transaction");

            // Assert
            assert_eq!(&values, expected, "{isolation}: {second_change}");
        }
    }
}

#[test]
fn should_preserve_repeatable_read_command_snapshots_across_own_changes() {
    // Arrange
    let fixture = sql_fixture(
        "rr-command-own-overlay",
        &[
            "CREATE TABLE rr_own_rows (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO rr_own_rows VALUES (1, 'base')",
        ],
    );
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin RW RR");
    fixture
        .execute("SELECT id FROM rr_own_rows")
        .expect("capture");
    let writer = fixture.cassie.create_session("tester", None);
    fixture
        .cassie
        .execute_sql(
            &writer,
            "INSERT INTO rr_own_rows VALUES (9, 'other')",
            Vec::new(),
        )
        .expect("other writer");
    fixture
        .execute("INSERT INTO rr_own_rows VALUES (2, 'own-before')")
        .expect("own insert");
    fixture.execute("SAVEPOINT before_cte").expect("savepoint");

    // Act
    let observed = fixture.execute("WITH changed AS (UPDATE rr_own_rows SET payload = 'own-after' WHERE id = 2 RETURNING id, payload), snapshot AS (SELECT id, payload FROM rr_own_rows) SELECT 'changed' AS source, id, payload FROM changed UNION ALL SELECT 'snapshot', id, payload FROM snapshot ORDER BY source, id").expect("command snapshot");
    let later = fixture
        .execute("SELECT id, payload FROM rr_own_rows ORDER BY id")
        .expect("later own changes");
    fixture
        .execute("ROLLBACK TO SAVEPOINT before_cte")
        .expect("restore own changes");
    let restored = fixture
        .execute("SELECT id, payload FROM rr_own_rows ORDER BY id")
        .expect("restored overlay");
    fixture.execute("COMMIT").expect("disjoint publication");
    let committed = fixture
        .execute("SELECT id, payload FROM rr_own_rows ORDER BY id")
        .expect("fresh committed view");

    // Assert
    assert_eq!(
        observed.rows(),
        &[
            vec![
                Value::String("changed".to_owned()),
                Value::Int64(2),
                Value::String("own-after".to_owned())
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(1),
                Value::String("base".to_owned())
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(2),
                Value::String("own-before".to_owned())
            ],
        ]
    );
    assert_eq!(
        later.rows(),
        &[
            vec![Value::Int64(1), Value::String("base".to_owned())],
            vec![Value::Int64(2), Value::String("own-after".to_owned())],
        ]
    );
    assert_eq!(
        restored.rows(),
        &[
            vec![Value::Int64(1), Value::String("base".to_owned())],
            vec![Value::Int64(2), Value::String("own-before".to_owned())],
        ]
    );
    assert_eq!(
        committed.rows(),
        &[
            vec![Value::Int64(1), Value::String("base".to_owned())],
            vec![Value::Int64(2), Value::String("own-before".to_owned())],
            vec![Value::Int64(9), Value::String("other".to_owned())],
        ]
    );
}
