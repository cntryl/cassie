//! Committed reservation probes and transaction overlays for #952.
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::types::Value;

fn seeded(label: &str, index: Option<&str>) -> SqlFixture {
    let mut setup = vec!["CREATE TABLE issue952 (id INT4, code TEXT, active BOOLEAN, CONSTRAINT issue952_pk PRIMARY KEY (id))".to_owned()];
    if let Some(index) = index {
        setup.push(index.to_owned());
    }
    let values = (1..=32)
        .map(|id| format!("({id}, 'value-{id}', TRUE)"))
        .collect::<Vec<_>>()
        .join(",");
    setup.push(format!("INSERT INTO issue952 VALUES {values}"));
    sql_fixture(label, &setup.iter().map(String::as_str).collect::<Vec<_>>())
}
const UNIQUE: &str = "CREATE UNIQUE INDEX issue952_code ON issue952(code)";

fn scans(fixture: &SqlFixture) -> u64 {
    fixture.cassie.midge.query_scan_entries_for_diagnostics()
}
fn assert_duplicate(error: CassieError, expected: &str) {
    let CassieError::UniqueViolation { constraint, .. } = error else {
        panic!("expected unique violation, got {error:?}");
    };
    assert_eq!(constraint, expected);
}

#[test]
fn should_probe_primary_reservation_given_new_key_when_committed_rows_exist() {
    // Arrange
    let fixture = seeded("issue952-primary-fresh", None);
    // Isolate validation from the once-per-commit cardinality refresh.
    fixture.execute("BEGIN").unwrap();
    let before = scans(&fixture);
    // Act
    fixture
        .execute("INSERT INTO issue952 VALUES (33, 'new', TRUE)")
        .expect("fresh key");
    let read_entries = scans(&fixture) - before;
    fixture.execute("COMMIT").expect("publish inserted row");
    let committed_entries = scans(&fixture) - before;
    // Assert
    assert!(
        committed_entries <= 33,
        "commit may refresh cardinality once: {committed_entries}"
    );
    assert_eq!(
        read_entries, 0,
        "primary-key validation must not scan committed rows"
    );
    assert_eq!(
        fixture
            .execute("SELECT code FROM issue952 WHERE id=33")
            .unwrap()
            .rows(),
        &[vec![Value::String("new".into())]]
    );
}

#[test]
fn should_probe_primary_reservation_given_duplicate_key_when_committed_rows_exist() {
    // Arrange
    let fixture = seeded("issue952-primary-conflict", None);
    let before = scans(&fixture);
    // Act
    let error = fixture
        .execute("INSERT INTO issue952 VALUES (1, 'new', TRUE)")
        .expect_err("duplicate primary key");
    let read_entries = scans(&fixture) - before;
    // Assert
    assert_duplicate(error, "issue952_pk");
    assert_eq!(
        read_entries, 0,
        "the reservation identifies the conflicting owner"
    );
}

#[test]
fn should_probe_unique_index_given_new_key_when_committed_rows_exist() {
    // Arrange
    let fixture = seeded("issue952-index-fresh", Some(UNIQUE));
    // Isolate validation from the once-per-commit cardinality refresh.
    fixture.execute("BEGIN").unwrap();
    let before = scans(&fixture);
    // Act
    fixture
        .execute("INSERT INTO issue952 VALUES (33, 'new', TRUE)")
        .expect("fresh index key");
    let read_entries = scans(&fixture) - before;
    fixture.execute("COMMIT").expect("publish inserted row");
    let committed_entries = scans(&fixture) - before;
    // Assert
    assert!(
        committed_entries <= 33,
        "commit may refresh cardinality once: {committed_entries}"
    );
    assert_eq!(
        read_entries, 0,
        "unique-index validation must not scan committed rows"
    );
}

#[test]
fn should_probe_unique_index_given_duplicate_key_when_committed_rows_exist() {
    // Arrange
    let fixture = seeded("issue952-index-conflict", Some(UNIQUE));
    let before = scans(&fixture);
    // Act
    let error = fixture
        .execute("INSERT INTO issue952 VALUES (33, 'value-1', TRUE)")
        .expect_err("duplicate index key");
    let read_entries = scans(&fixture) - before;
    // Assert
    assert_duplicate(error, "issue952_code");
    assert_eq!(
        read_entries, 0,
        "the index reservation identifies the conflicting owner"
    );
}

#[test]
fn should_reconcile_committed_owners_given_staged_delete_and_rekey_when_keys_are_reused() {
    // Arrange
    let fixture = seeded("issue952-staged-reuse", Some(UNIQUE));
    fixture.execute("BEGIN").unwrap();
    fixture.execute("DELETE FROM issue952 WHERE id=1").unwrap();
    fixture
        .execute("UPDATE issue952 SET code='changed' WHERE id=2")
        .unwrap();
    // Act
    fixture
        .execute("INSERT INTO issue952 VALUES (101,'value-1',TRUE),(102,'value-2',TRUE)")
        .expect("staged owners release old keys");
    fixture
        .execute("COMMIT")
        .expect("atomic reservation publication");
    // Assert
    assert_eq!(
        fixture
            .execute("SELECT id,code FROM issue952 WHERE id IN (1,2,101,102) ORDER BY id")
            .unwrap()
            .rows(),
        &[
            vec![Value::Int64(2), Value::String("changed".into())],
            vec![Value::Int64(101), Value::String("value-1".into())],
            vec![Value::Int64(102), Value::String("value-2".into())],
        ]
    );
}

#[test]
fn should_reject_staged_collision_given_uncommitted_owner_when_transaction_rolls_back() {
    // Arrange
    let fixture = seeded("issue952-staged-collision", Some(UNIQUE));
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("INSERT INTO issue952 VALUES (101,'staged',TRUE)")
        .unwrap();
    // Act
    let error = fixture
        .execute("INSERT INTO issue952 VALUES (102,'staged',TRUE)")
        .expect_err("staged index conflict");
    fixture
        .execute("ROLLBACK")
        .expect("rollback failed transaction");
    // Assert
    assert_duplicate(error, "issue952_code");
    assert_eq!(
        fixture
            .execute("SELECT count(*) FROM issue952 WHERE id>32")
            .unwrap()
            .rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_exclude_updated_owner_given_unchanged_unique_key_when_row_is_updated() {
    // Arrange
    let fixture = seeded("issue952-excluded-owner", Some(UNIQUE));
    // Act
    fixture
        .execute("UPDATE issue952 SET code='value-1', active=FALSE WHERE id=1")
        .expect("owner may retain its key");
    // Assert
    assert_eq!(
        fixture
            .execute("SELECT code,active FROM issue952 WHERE id=1")
            .unwrap()
            .rows(),
        &[vec![Value::String("value-1".into()), Value::Bool(false)]]
    );
}

#[test]
fn should_preserve_partial_predicate_given_nonindexed_value_when_duplicate_is_inserted() {
    // Arrange
    let fixture = seeded(
        "issue952-partial",
        Some("CREATE UNIQUE INDEX issue952_code ON issue952(code) WHERE active"),
    );
    // Act
    fixture
        .execute("INSERT INTO issue952 VALUES (101,'value-1',FALSE)")
        .expect("nonindexed duplicate does not conflict");
    // Assert
    assert_eq!(
        fixture
            .execute("SELECT count(*) FROM issue952 WHERE id>32")
            .unwrap()
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_distinct_null_keys_given_unique_index_when_nulls_are_inserted() {
    // Arrange
    let fixture = seeded("issue952-nulls", Some(UNIQUE));
    // Act
    fixture
        .execute("INSERT INTO issue952 VALUES (101,NULL,TRUE),(102,NULL,TRUE)")
        .expect("NULL keys do not conflict");
    // Assert
    assert_eq!(
        fixture
            .execute("SELECT count(*) FROM issue952 WHERE id>32")
            .unwrap()
            .rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_preserve_canonical_char_key_given_trailing_blanks_when_index_owner_exists() {
    // Arrange
    let fixture = sql_fixture(
        "issue952-char-key",
        &[
            "CREATE TABLE issue952_char (id INT4 PRIMARY KEY, code CHAR(4))",
            "CREATE UNIQUE INDEX issue952_char_code ON issue952_char(code)",
            "INSERT INTO issue952_char VALUES (1,'x')",
        ],
    );
    // Act
    let error = fixture
        .execute("INSERT INTO issue952_char VALUES (2,'x ')")
        .expect_err("CHAR canonical key conflict");
    // Assert
    assert_duplicate(error, "issue952_char_code");
}

fn plain_unique(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue952_plain (code TEXT, CONSTRAINT issue952_plain_key UNIQUE (code))",
            "INSERT INTO issue952_plain VALUES ('existing')",
        ],
    )
}

#[test]
fn should_preserve_plain_unique_given_committed_owner_when_duplicate_is_inserted() {
    // Arrange
    let fixture = plain_unique("issue952-plain-committed");
    // Act
    let error = fixture
        .execute("INSERT INTO issue952_plain VALUES ('existing')")
        .expect_err("plain UNIQUE remains enforced");
    // Assert
    assert_duplicate(error, "issue952_plain_key");
    assert_eq!(
        fixture
            .execute("SELECT count(*) FROM issue952_plain")
            .unwrap()
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_plain_unique_given_staged_owner_when_duplicate_is_inserted() {
    // Arrange
    let fixture = plain_unique("issue952-plain-staged");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("INSERT INTO issue952_plain VALUES ('staged')")
        .unwrap();
    // Act
    let error = fixture
        .execute("INSERT INTO issue952_plain VALUES ('staged')")
        .expect_err("staged plain UNIQUE remains enforced");
    fixture.execute("ROLLBACK").unwrap();
    // Assert
    assert_duplicate(error, "issue952_plain_key");
    assert_eq!(
        fixture
            .execute("SELECT count(*) FROM issue952_plain")
            .unwrap()
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_plain_unique_nulls_given_unbacked_constraint_when_fresh_values_are_inserted() {
    // Arrange
    let fixture = plain_unique("issue952-plain-nulls");
    // Act
    fixture
        .execute("INSERT INTO issue952_plain VALUES (NULL),(NULL),('fresh')")
        .expect("distinct NULL and fresh UNIQUE values");
    // Assert
    assert_eq!(
        fixture
            .execute("SELECT count(*) FROM issue952_plain")
            .unwrap()
            .rows(),
        &[vec![Value::Int64(4)]]
    );
}

#[test]
fn should_evaluate_partial_predicate_given_null_key_when_insert_is_staged() {
    // Arrange
    let fixture = seeded(
        "issue952-null-predicate",
        Some("CREATE UNIQUE INDEX issue952_code ON issue952(code) WHERE 1/id>0"),
    );
    fixture.execute("BEGIN").unwrap();
    // Act
    let result = fixture.execute("INSERT INTO issue952 VALUES (0,NULL,TRUE)");
    fixture.execute("ROLLBACK").unwrap();
    // Assert
    assert!(
        result.is_err(),
        "NULL keys must not elide evaluation of the partial predicate"
    );
    assert_eq!(
        fixture
            .execute("SELECT count(*) FROM issue952 WHERE id=0")
            .unwrap()
            .rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_preserve_primary_validation_given_missing_physical_index_when_owner_exists() {
    // Arrange
    let fixture = seeded("issue952-missing-primary", None);
    fixture
        .cassie
        .midge
        .delete_index("issue952", "issue952_pkey")
        .unwrap();
    // Act
    let error = fixture
        .execute("INSERT INTO issue952 VALUES (1,'new',TRUE)")
        .expect_err("logical primary constraint remains enforced");
    // Assert
    assert_duplicate(error, "issue952_pk");
}

#[test]
fn should_preserve_index_validation_given_missing_physical_index_when_owner_exists() {
    // Arrange
    let fixture = seeded("issue952-missing-index", Some(UNIQUE));
    fixture
        .cassie
        .midge
        .delete_index("issue952", "issue952_code")
        .unwrap();
    // Act
    let error = fixture
        .execute("INSERT INTO issue952 VALUES (33,'value-1',TRUE)")
        .expect_err("logical unique index remains enforced");
    // Assert
    assert_duplicate(error, "issue952_code");
}

#[test]
fn should_preserve_index_validation_given_divergent_storage_definition_when_owner_exists() {
    // Arrange
    let fixture = seeded("issue952-divergent-index", Some(UNIQUE));
    let mut stored = fixture
        .cassie
        .catalog
        .get_index("issue952", "issue952_code")
        .unwrap();
    fixture
        .cassie
        .midge
        .delete_index("issue952", "issue952_code")
        .unwrap();
    stored.field = "id".to_owned();
    stored.fields = vec!["id".to_owned()];
    fixture.cassie.midge.put_index(&stored).unwrap();
    // Act
    let error = fixture
        .execute("INSERT INTO issue952 VALUES (33,'value-1',TRUE)")
        .expect_err("catalog key definition remains authoritative");
    // Assert
    assert_duplicate(error, "issue952_code");
}
