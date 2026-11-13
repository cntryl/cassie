//! Every `ALTER TABLE` form a row table supports behaves identically on a
//! column table, keeps the column layout, and is transactional and durable.
//!
//! Each parity test runs the same statements against a row table and a column
//! table holding the same data and compares outcomes, SQLSTATEs and rows.

use super::{env, Env};
use cassie::types::Value;
use std::path::PathBuf;

const FIELDS: usize = 3;
const READ_ALL: &str = "SELECT id, name, qty FROM t ORDER BY id";

type Outcome = Result<(), (String, String)>;

struct Pair {
    row: Env,
    col: Env,
}

fn outcome(env: &Env, sql: &str) -> Outcome {
    env.cassie
        .execute_sql(&env.session, sql, vec![])
        .map(|_| ())
        .map_err(|error| (error.sql_state().to_owned(), error.to_string()))
}

fn seeded_env(suffix: &str) -> Env {
    let env = env();
    env.run(&format!(
        "CREATE TABLE t (id INT PRIMARY KEY, name TEXT, qty INT){suffix}"
    ));
    env.run("CREATE TABLE parent (id INT PRIMARY KEY)");
    env.run("INSERT INTO parent VALUES (10), (20)");
    env.run("INSERT INTO t VALUES (1, 'one', 10), (2, 'two', 20), (3, NULL, 30)");
    env
}

fn pair() -> Pair {
    Pair {
        row: seeded_env(""),
        col: seeded_env(" WITH (storage = column_store)"),
    }
}

impl Pair {
    /// Runs `sql` on both tables, asserts equal outcomes and returns whether it succeeded.
    fn run(&self, sql: &str) -> bool {
        let row = outcome(&self.row, sql);
        let col = outcome(&self.col, sql);
        assert_eq!(row, col, "row and column outcomes differ for: {sql}");
        row.is_ok()
    }

    /// Reads from both tables, asserts equal rows and returns them.
    fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        let row = self.row.rows(sql);
        let col = self.col.rows(sql);
        assert_eq!(row, col, "row and column rows differ for: {sql}");
        col
    }

    fn assert_layout(&self, fields: usize, step: &str) {
        let live = self.col.rows("SELECT id FROM t").len();
        self.col.assert_column_authority("t", live, fields, step);
    }
}

#[test]
fn should_add_column_with_constant_default_on_both_table_kinds() {
    // Arrange
    let pair = pair();

    // Act
    let added = pair.run("ALTER TABLE t ADD COLUMN extra INT DEFAULT 7");
    let rows = pair.rows("SELECT id, extra FROM t ORDER BY id");

    // Assert
    assert!(added);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row[1] == rows[0][1]));
    assert_ne!(rows[0][1], Value::Null);
    pair.assert_layout(FIELDS + 1, "add column with default");
}

#[test]
fn should_add_column_without_default_and_accept_later_writes() {
    // Arrange
    let pair = pair();

    // Act
    let added = pair.run("ALTER TABLE t ADD COLUMN extra INT");
    let before = pair.rows("SELECT id, extra FROM t ORDER BY id");
    pair.run("UPDATE t SET extra = 5 WHERE id = 1");
    pair.run("INSERT INTO t VALUES (4, 'four', 40, 9)");
    let after = pair.rows("SELECT id, extra FROM t ORDER BY id");

    // Assert
    assert!(added);
    assert!(before.iter().all(|row| row[1] == Value::Null));
    assert_eq!(after.len(), 4);
    assert_ne!(after[0][1], Value::Null);
}

#[test]
fn should_backfill_volatile_default_on_both_table_kinds() {
    // Arrange
    let pair = pair();

    // Act
    let added = pair.run("ALTER TABLE t ADD COLUMN made TIMESTAMPTZ DEFAULT now()");
    let filled = pair.rows("SELECT count(*) FROM t WHERE made IS NOT NULL");

    // Assert
    assert!(added);
    assert_eq!(filled, vec![vec![Value::Int64(3)]]);
}

#[test]
fn should_match_row_tables_for_add_column_not_null() {
    // Arrange
    let pair = pair();

    // Act
    let without_default = pair.run("ALTER TABLE t ADD COLUMN req INT NOT NULL");
    let with_default = pair.run("ALTER TABLE t ADD COLUMN req2 INT NOT NULL DEFAULT 1");
    let rows = pair.rows("SELECT id, req2 FROM t ORDER BY id");

    // Assert
    assert!(!without_default);
    assert!(with_default);
    assert_eq!(rows.len(), 3);
}

#[test]
fn should_drop_column_and_reclaim_its_field_keys() {
    // Arrange
    let pair = pair();

    // Act
    let dropped = pair.run("ALTER TABLE t DROP COLUMN name");
    let rows = pair.rows("SELECT id, qty FROM t ORDER BY id");
    let missing = pair.run("SELECT name FROM t");
    let again = pair.run("ALTER TABLE t DROP COLUMN name");

    // Assert
    assert!(dropped);
    assert_eq!(rows.len(), 3);
    assert!(!missing);
    assert!(!again);
    pair.assert_layout(FIELDS - 1, "drop column");
}

#[test]
fn should_rename_column_without_rewriting_field_keys() {
    // Arrange
    let pair = pair();
    let before = pair.col.column_entries("t").len();

    // Act
    let renamed = pair.run("ALTER TABLE t RENAME COLUMN name TO label");
    let rows = pair.rows("SELECT id, label, qty FROM t ORDER BY id");
    let old_name = pair.run("SELECT name FROM t");

    // Assert
    assert!(renamed);
    assert_eq!(rows.len(), 3);
    assert!(!old_name);
    assert_eq!(pair.col.column_entries("t").len(), before);
    pair.assert_layout(FIELDS, "rename column");
}

#[test]
fn should_retype_column_and_keep_column_authority() {
    // Arrange
    let pair = pair();

    // Act
    let retyped = pair.run("ALTER TABLE t ALTER COLUMN qty TYPE TEXT USING qty::TEXT || 'x'");
    let rows = pair.rows("SELECT id, qty, pg_typeof(qty)::text FROM t ORDER BY id");

    // Assert
    assert!(retyped);
    assert_eq!(rows.len(), 3);
    pair.assert_layout(FIELDS, "alter column type");
}

#[test]
fn should_retype_column_without_using_clause() {
    // Arrange
    let pair = pair();

    // Act
    let retyped = pair.run("ALTER TABLE t ALTER COLUMN qty TYPE BIGINT USING qty::BIGINT");
    let rows = pair.rows("SELECT id, qty FROM t ORDER BY id");

    // Assert
    assert!(retyped);
    assert_eq!(rows.len(), 3);
    pair.assert_layout(FIELDS, "alter column type without using");
}

#[test]
fn should_leave_data_unchanged_when_a_retype_cast_fails() {
    // Arrange
    let pair = pair();
    let before = pair.rows(READ_ALL);

    // Act
    let retyped = pair.run("ALTER TABLE t ALTER COLUMN name TYPE INT USING name::INT");
    let after = pair.rows(READ_ALL);

    // Assert
    assert!(!retyped);
    assert_eq!(after, before);
    pair.assert_layout(FIELDS, "failed alter column type");
}

#[test]
fn should_never_write_row_blobs_for_a_column_table_across_alter_forms() {
    // Arrange
    let env = seeded_env(" WITH (storage = column_store)");

    // Act
    env.run("ALTER TABLE t ALTER COLUMN qty TYPE BIGINT USING qty::BIGINT");
    env.run("ALTER TABLE t ADD COLUMN extra INT DEFAULT 1");
    env.run("ALTER TABLE t RENAME COLUMN extra TO more");
    env.run("ALTER TABLE t DROP COLUMN more");

    // Assert
    assert!(env.row_store_keys("t").is_empty());
    env.assert_column_authority("t", 3, FIELDS, "all alter forms");
}

#[test]
fn should_match_row_tables_for_default_and_not_null_changes() {
    // Arrange
    let pair = pair();

    // Act
    let set_default = pair.run("ALTER TABLE t ALTER COLUMN qty SET DEFAULT 5");
    pair.run("INSERT INTO t (id, name) VALUES (4, 'four')");
    let drop_default = pair.run("ALTER TABLE t ALTER COLUMN qty DROP DEFAULT");
    pair.run("INSERT INTO t (id, name) VALUES (5, 'five')");
    let not_null_with_nulls = pair.run("ALTER TABLE t ALTER COLUMN name SET NOT NULL");
    pair.run("UPDATE t SET name = 'three' WHERE id = 3");
    let not_null = pair.run("ALTER TABLE t ALTER COLUMN name SET NOT NULL");
    let violating = pair.run("INSERT INTO t VALUES (6, NULL, 1)");
    let drop_not_null = pair.run("ALTER TABLE t ALTER COLUMN name DROP NOT NULL");
    let rows = pair.rows(READ_ALL);

    // Assert
    assert!(set_default && drop_default && not_null && drop_not_null);
    assert!(!not_null_with_nulls && !violating);
    assert_eq!(rows.len(), 5);
    pair.assert_layout(FIELDS, "default and not null");
}

#[test]
fn should_validate_added_constraints_by_scanning_column_values() {
    // Arrange
    let pair = pair();

    // Act
    let check_fails = pair.run("ALTER TABLE t ADD CONSTRAINT big CHECK (qty > 15)");
    let check_ok = pair.run("ALTER TABLE t ADD CONSTRAINT pos CHECK (qty > 0)");
    let check_violation = pair.run("INSERT INTO t VALUES (7, 'x', -1)");
    let unique_ok = pair.run("ALTER TABLE t ADD CONSTRAINT uq UNIQUE (name)");
    let unique_violation = pair.run("INSERT INTO t VALUES (8, 'one', 10)");
    pair.run("INSERT INTO t (id, name, qty) VALUES (10, 'dup', 20), (11, 'dup2', 20)");
    let fk_fails =
        pair.run("ALTER TABLE t ADD CONSTRAINT fk_bad FOREIGN KEY (qty) REFERENCES parent (id)");
    pair.run("UPDATE t SET qty = 10 WHERE id = 3");
    let fk_ok =
        pair.run("ALTER TABLE t ADD CONSTRAINT fk_ok FOREIGN KEY (qty) REFERENCES parent (id)");
    let fk_violation = pair.run("INSERT INTO t VALUES (9, 'y', 99)");
    let dropped = pair.run("ALTER TABLE t DROP CONSTRAINT pos");

    // Assert
    assert!(check_ok && unique_ok && fk_ok && dropped);
    assert!(!check_fails && !check_violation && !fk_fails && !fk_violation);
    assert!(!unique_violation);
    pair.assert_layout(FIELDS, "constraints");
}

#[test]
fn should_roll_back_alter_forms_staged_in_a_transaction() {
    // Arrange
    let pair = pair();
    let before = pair.rows(READ_ALL);

    // Act
    pair.run("BEGIN");
    pair.run("ALTER TABLE t ADD COLUMN extra INT DEFAULT 3");
    pair.run("ALTER TABLE t ALTER COLUMN qty TYPE TEXT USING qty::TEXT");
    pair.run("ALTER TABLE t RENAME COLUMN name TO label");
    pair.run("ALTER TABLE t DROP COLUMN label");
    let staged = pair.rows("SELECT id, qty, extra FROM t ORDER BY id");
    pair.run("ROLLBACK");
    let after = pair.rows(READ_ALL);

    // Assert
    assert_eq!(staged.len(), 3);
    assert_eq!(after, before);
    pair.assert_layout(FIELDS, "rolled back alter forms");
}

#[test]
fn should_commit_alter_forms_staged_in_a_transaction() {
    // Arrange
    let pair = pair();

    // Act
    pair.run("BEGIN");
    pair.run("ALTER TABLE t ADD COLUMN extra INT DEFAULT 3");
    pair.run("ALTER TABLE t ALTER COLUMN qty TYPE TEXT USING qty::TEXT");
    pair.run("COMMIT");
    let rows = pair.rows("SELECT id, qty, extra FROM t ORDER BY id");

    // Assert
    assert_eq!(rows.len(), 3);
    pair.assert_layout(FIELDS + 1, "committed alter forms");
}

#[test]
fn should_undo_only_the_alter_after_a_savepoint_rollback() {
    // Arrange
    let pair = pair();

    // Act
    pair.run("BEGIN");
    pair.run("ALTER TABLE t ADD COLUMN kept INT DEFAULT 1");
    pair.run("SAVEPOINT s");
    pair.run("ALTER TABLE t DROP COLUMN name");
    pair.run("ALTER TABLE t ALTER COLUMN qty TYPE TEXT USING qty::TEXT");
    pair.run("ROLLBACK TO s");
    pair.run("COMMIT");
    let rows = pair.rows("SELECT id, name, qty, kept FROM t ORDER BY id");

    // Assert
    assert_eq!(rows.len(), 3);
    pair.assert_layout(FIELDS + 1, "savepoint rollback");
}

struct Local(PathBuf);

impl Local {
    fn new() -> Self {
        crate::support_sql::use_local_storage();
        let path = PathBuf::from(crate::support_sql::data_dir("column-alter-restart"));
        std::fs::create_dir_all(&path).expect("isolated data directory");
        Self(path)
    }

    fn open(&self) -> Env {
        let cassie = cassie::app::Cassie::new_with_data_dir(&self.0).expect("local store");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        Env { cassie, session }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            assert!(std::thread::panicking(), "fixture cleanup: {error}");
        }
    }
}

#[test]
fn should_keep_altered_column_tables_across_restart() {
    // Arrange
    let local = Local::new();
    let expected = {
        let env = local.open();
        env.run(
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT, qty INT) WITH (storage = column_store)",
        );
        env.run("INSERT INTO t VALUES (1, 'one', 10), (2, 'two', 20), (3, NULL, 30)");
        env.run("ALTER TABLE t ADD COLUMN extra INT DEFAULT 4");
        env.run("ALTER TABLE t ALTER COLUMN qty TYPE TEXT USING qty::TEXT");
        env.run("ALTER TABLE t RENAME COLUMN name TO label");
        env.run("ALTER TABLE t ADD COLUMN gone INT");
        env.run("ALTER TABLE t DROP COLUMN gone");
        env.rows("SELECT id, label, qty, extra FROM t ORDER BY id")
    };

    // Act
    let reopened = local.open();
    let rows = reopened.rows("SELECT id, label, qty, extra FROM t ORDER BY id");

    // Assert
    assert_eq!(rows, expected);
    reopened.assert_column_authority("t", 3, FIELDS + 1, "after restart");
}
