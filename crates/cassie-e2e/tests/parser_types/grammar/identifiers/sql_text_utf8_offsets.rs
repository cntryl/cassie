use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::executor::QueryResult;
use cassie::types::Value;

use support::data_dir;

fn with_session(label: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    let path = data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        test(&cassie, &session);
    });
    let _ = std::fs::remove_dir_all(path);
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> QueryResult {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result,
        Err(error) => panic!("statement failed: {sql}: {error}"),
    }
}

#[test]
fn should_filter_on_non_ascii_string_literals_without_panicking() {
    with_session("utf8_literal_filter", |cassie, session| {
        // Arrange
        let names = ["café", "Müller", "naïve", "日本語", "straße", "😀"];
        run(cassie, session, "CREATE TABLE utf8_names (name TEXT)");
        for name in names {
            run(
                cassie,
                session,
                &format!("INSERT INTO utf8_names (name) VALUES ('{name}')"),
            );
        }

        // Act
        let matched = names.map(|name| {
            run(
                cassie,
                session,
                &format!("SELECT name FROM utf8_names WHERE name = '{name}'"),
            )
            .rows()
            .to_vec()
        });

        // Assert
        for (rows, name) in matched.iter().zip(names) {
            assert_eq!(rows, &vec![vec![Value::String(name.to_string())]]);
        }
    });
}

#[test]
fn should_keep_clause_boundaries_when_lowercasing_changes_byte_length() {
    with_session("utf8_lowercase_offsets", |cassie, session| {
        // Arrange
        run(cassie, session, "CREATE TABLE utf8_offsets (a TEXT)");
        run(
            cassie,
            session,
            "INSERT INTO utf8_offsets (a) VALUES ('\u{212A}é')",
        );

        // Act
        let aliased = run(cassie, session, "SELECT 'İé' AS x FROM utf8_offsets");
        let limited = run(
            cassie,
            session,
            "SELECT a FROM utf8_offsets WHERE a = '\u{212A}é' LIMIT 1",
        );

        // Assert
        assert_eq!(aliased.columns()[0].name, "x");
        assert_eq!(
            limited.rows(),
            vec![vec![Value::String("\u{212A}é".to_string())]]
        );
    });
}

#[test]
fn should_rename_columns_whose_lowercase_changes_byte_length() {
    with_session("utf8_rename_column", |cassie, session| {
        // Arrange
        run(cassie, session, "CREATE TABLE utf8_rename (\"İa\" TEXT)");
        run(
            cassie,
            session,
            "INSERT INTO utf8_rename (\"İa\") VALUES ('v')",
        );

        // Act
        run(
            cassie,
            session,
            "ALTER TABLE utf8_rename RENAME COLUMN \"İa\" TO b",
        );
        let selected = run(cassie, session, "SELECT b FROM utf8_rename");

        // Assert
        assert_eq!(selected.rows(), vec![vec![Value::String("v".to_string())]]);
    });
}

#[test]
fn should_create_tables_whose_names_contain_multibyte_characters() {
    with_session("utf8_table_names", |cassie, session| {
        // Arrange
        let statements = [
            "CREATE TABLE ét (id BIGINT)",
            "CREATE TABLE \"éh\" (id BIGINT, note VARCHAR(20))",
        ];

        // Act
        let created = statements.map(|sql| run(cassie, session, sql).command().to_owned());
        run(
            cassie,
            session,
            "INSERT INTO \"éh\" (id, note) VALUES (1, 'n')",
        );
        let selected = run(cassie, session, "SELECT note FROM \"éh\"");

        // Assert
        assert_eq!(created, ["CREATE TABLE", "CREATE TABLE"]);
        assert_eq!(selected.rows(), vec![vec![Value::String("n".to_string())]]);
    });
}
