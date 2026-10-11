//! Finite RETURNING scalar-phase fixture and literal metadata assertions.
use crate::support_relational_qualification::{fixture, Fixture};
use cassie::executor::{ColumnMeta, QueryResult};
use cassie::types::Value;

pub fn seeded_fixture() -> Fixture {
    let fixture = fixture();
    for sql in [
        "CREATE TABLE rt(id BIGINT)",
        "CREATE TABLE ri(id BIGINT)",
        "INSERT INTO rt VALUES(1),(2)",
        "INSERT INTO ri VALUES(1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("RETURNING phase seed");
    }
    fixture
}

pub fn column(name: &str, boolean: bool) -> ColumnMeta {
    ColumnMeta {
        name: name.into(),
        data_type: if boolean { "boolean" } else { "bigint" }.into(),
        type_oid: if boolean { 16 } else { 20 },
        typlen: if boolean { 1 } else { 8 },
        atttypmod: -1,
        format_code: 0,
        nullable: true,
    }
}

pub fn assert_result(mut actual: QueryResult, boolean: bool, empty: bool) {
    let mut columns = Vec::new();
    if !empty {
        columns.push(column("id", false));
    }
    columns.push(column("selected", boolean));
    assert_eq!(actual.columns, columns, "literal static descriptors");
    assert_eq!(actual.command, if empty { "UPDATE 0" } else { "UPDATE 2" });
    actual.rows.sort_by_key(|row| match row.first() {
        Some(Value::Int64(id)) => *id,
        other => panic!("BIGINT id: {other:?}"),
    });
    let expected = if empty {
        vec![]
    } else {
        let selected = if boolean {
            Value::Bool(false)
        } else {
            Value::Int64(1)
        };
        vec![
            vec![Value::Int64(1), selected.clone()],
            vec![Value::Int64(2), selected],
        ]
    };
    assert_eq!(actual.rows, expected, "exact returned multiset");
}

pub fn assert_original_rows(fixture: &Fixture) {
    let observer = fixture.cassie.create_session("phase-observer", None);
    let actual = fixture
        .cassie
        .execute_sql(&observer, "SELECT id FROM rt ORDER BY id", vec![])
        .expect("independent committed readback");
    assert_eq!(actual.columns, vec![column("id", false)]);
    assert_eq!(actual.command, "SELECT");
    assert_eq!(
        actual.rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}
