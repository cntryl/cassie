//! No ingress path may write a row blob or legacy document key for a column
//! table, and every live row keeps a marker plus one key per field.

use super::{env, Env};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::{DataType, FieldSchema};

const TABLE: &str = "ingress";
const FIELDS: usize = 3;

fn seeded() -> Env {
    let env = env();
    env.run("CREATE TABLE ingress (id INT PRIMARY KEY, name TEXT, qty INT) WITH (storage = column_store)");
    env.run("CREATE TABLE ingress_src (id INT, name TEXT, qty INT)");
    env.run("INSERT INTO ingress VALUES (1, 'one', 10), (2, 'two', 20), (3, 'three', 30)");
    env
}

#[test]
fn should_keep_column_authority_after_insert() {
    // Arrange
    let env = env();
    env.run("CREATE TABLE ingress (id INT PRIMARY KEY, name TEXT, qty INT) WITH (storage = column_store)");

    // Act
    env.run("INSERT INTO ingress VALUES (1, 'one', 10), (2, 'two', 20)");

    // Assert
    env.assert_column_authority(TABLE, 2, FIELDS, "insert");
}

#[test]
fn should_keep_column_authority_after_update_or_delete() {
    // Arrange
    let env = seeded();

    // Act
    env.run("UPDATE ingress SET qty = qty + 1 WHERE id = 1");
    let after_update = env.rows("SELECT qty FROM ingress WHERE id = 1");
    env.run("DELETE FROM ingress WHERE id = 3");

    // Assert
    assert_eq!(after_update, vec![vec![cassie::types::Value::Int64(11)]]);
    env.assert_column_authority(TABLE, 2, FIELDS, "update and delete");
}

#[test]
fn should_keep_column_authority_after_on_conflict_upsert() {
    // Arrange
    let env = seeded();

    // Act
    env.run("INSERT INTO ingress VALUES (1, 'uno', 11), (4, 'four', 40) ON CONFLICT (id) DO UPDATE SET name = excluded.name, qty = excluded.qty");
    env.run("INSERT INTO ingress VALUES (2, 'dos', 22) ON CONFLICT (id) DO NOTHING");

    // Assert
    env.assert_column_authority(TABLE, 4, FIELDS, "on conflict");
}

#[test]
fn should_keep_column_authority_after_insert_select() {
    // Arrange
    let env = seeded();
    env.run("INSERT INTO ingress_src VALUES (7, 'seven', 70), (8, 'eight', 80)");

    // Act
    env.run("INSERT INTO ingress SELECT id, name, qty FROM ingress_src");

    // Assert
    env.assert_column_authority(TABLE, 5, FIELDS, "insert select");
}

#[test]
fn should_keep_column_authority_after_copy_from() {
    // Arrange
    let env = seeded();
    let statement = CopyStatement {
        table: TABLE.to_string(),
        columns: vec!["id".to_string(), "name".to_string(), "qty".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };

    // Act
    let copied =
        env.cassie
            .copy_from_csv_stdin(&env.session, &statement, b"10,ten,100\n11,eleven,110\n");

    // Assert
    assert_eq!(copied.expect("copy"), 2);
    env.assert_column_authority(TABLE, 5, FIELDS, "copy from");
}

#[test]
fn should_keep_column_authority_after_truncate() {
    // Arrange
    let env = seeded();

    // Act
    env.run("TRUNCATE ingress");
    env.run("INSERT INTO ingress VALUES (9, 'nine', 90)");

    // Assert
    env.assert_column_authority(TABLE, 1, FIELDS, "truncate");
}

#[test]
fn should_keep_column_authority_after_an_explicit_transaction() {
    // Arrange
    let env = seeded();

    // Act
    env.run("BEGIN");
    env.run("INSERT INTO ingress VALUES (5, 'five', 50)");
    env.run("UPDATE ingress SET name = 'ONE' WHERE id = 1");
    env.run("DELETE FROM ingress WHERE id = 2");
    env.run("COMMIT");

    // Assert
    env.assert_column_authority(TABLE, 3, FIELDS, "explicit transaction");
}

#[test]
fn should_keep_column_authority_across_column_add_drop() {
    // Arrange
    let env = seeded();
    let midge = &env.cassie.midge;

    // Act
    midge
        .alter_collection_add_column(
            TABLE,
            FieldSchema {
                name: "extra".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        )
        .expect("add column");
    let old_rows_validate = midge
        .verify_column_store_for_diagnostics(TABLE)
        .expect("verifier");
    midge
        .put_document(
            TABLE,
            Some("fresh".to_string()),
            serde_json::json!({"id": 99, "name": "fresh", "qty": 1, "extra": "x"}),
        )
        .expect("write after add column");
    let widened = env.column_entries(TABLE).len();
    midge
        .alter_collection_drop_column(TABLE, "qty")
        .expect("drop column");

    // Assert
    assert!(old_rows_validate.is_empty(), "{old_rows_validate:?}");
    assert_eq!(env.row_store_keys(TABLE), Vec::<Vec<u8>>::new());
    // 3 old rows x 3 fields + 1 marker each, plus the fresh row: 4 fields + 1 marker.
    assert_eq!(widened, 3 * (3 + 1) + (4 + 1));
    let dropped_field = b"f\0\0\0\0\x03\0";
    assert!(env.column_entries(TABLE).iter().all(|(key, _)| {
        !key.windows(dropped_field.len())
            .any(|window| window == dropped_field)
    }));
    assert_eq!(
        midge
            .verify_column_store_for_diagnostics(TABLE)
            .expect("verifier"),
        Vec::<String>::new()
    );
}

#[test]
fn should_reject_fresh_loaders_for_a_column_table() {
    // Arrange
    let env = seeded();
    let midge = &env.cassie.midge;
    let document = || vec![(Some("f".to_string()), serde_json::json!({"id": 50}))];

    // Act
    let fresh = midge.put_fresh_documents(TABLE, document());
    let time_series = midge.put_fresh_time_series_documents(TABLE, document());
    let graph = midge.put_fresh_graph_documents(TABLE, document());

    // Assert
    assert!(fresh.is_err());
    assert!(time_series.is_err());
    assert!(graph.is_err());
    env.assert_column_authority(TABLE, 3, FIELDS, "fresh loaders");
}
