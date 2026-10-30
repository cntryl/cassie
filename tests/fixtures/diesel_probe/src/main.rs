use diesel::deserialize::{self, QueryableByName};
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error};
use diesel::row::NamedRow;
use diesel::sql_types::{BigInt, Integer, Text};

struct TextRow {
    value: String,
}

struct CountRow {
    value: i64,
}

struct IntegerRow {
    value: i32,
}

struct JoinedRow {
    title: String,
    status: String,
}

impl QueryableByName<Pg> for TextRow {
    fn build<'a>(row: &impl NamedRow<'a, Pg>) -> deserialize::Result<Self> {
        Ok(Self {
            value: NamedRow::get::<Text, String>(row, "value")?,
        })
    }
}

impl QueryableByName<Pg> for CountRow {
    fn build<'a>(row: &impl NamedRow<'a, Pg>) -> deserialize::Result<Self> {
        Ok(Self {
            value: NamedRow::get::<BigInt, i64>(row, "value")?,
        })
    }
}

impl QueryableByName<Pg> for IntegerRow {
    fn build<'a>(row: &impl NamedRow<'a, Pg>) -> deserialize::Result<Self> {
        Ok(Self {
            value: NamedRow::get::<Integer, i32>(row, "value")?,
        })
    }
}

impl QueryableByName<Pg> for JoinedRow {
    fn build<'a>(row: &impl NamedRow<'a, Pg>) -> deserialize::Result<Self> {
        Ok(Self {
            title: NamedRow::get::<Text, String>(row, "title")?,
            status: NamedRow::get::<Text, String>(row, "status")?,
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("expected PostgreSQL connection URL")?;
    let connection = &mut PgConnection::establish(&url)?;

    create_probe_data(connection)?;

    let catalog = diesel::sql_query(
        "SELECT table_name AS value FROM information_schema.tables WHERE table_name = $1",
    )
    .bind::<Text, _>("compat_diesel_probe")
    .get_result::<TextRow>(connection)?
    .value;
    let prepared =
        diesel::sql_query("SELECT title AS value FROM compat_diesel_probe WHERE id = $1")
            .bind::<Integer, _>(1_i32)
            .get_result::<TextRow>(connection)?
            .value;
    let joined = diesel::sql_query(
        "SELECT p.title AS title, s.status AS status \
         FROM compat_diesel_probe p JOIN compat_diesel_status s ON p.id = s.id \
         WHERE p.id = $1",
    )
    .bind::<Integer, _>(1_i32)
    .get_result::<JoinedRow>(connection)?;

    let rollback = connection.transaction::<(), Error, _>(|connection| {
        diesel::sql_query("INSERT INTO compat_diesel_probe (id, title) VALUES ($1, $2)")
            .bind::<Integer, _>(2_i32)
            .bind::<Text, _>("beta")
            .execute(connection)?;
        Err(Error::RollbackTransaction)
    });
    assert!(matches!(rollback, Err(Error::RollbackTransaction)));
    let row_count = diesel::sql_query("SELECT COUNT(*) AS value FROM compat_diesel_probe")
        .get_result::<CountRow>(connection)?
        .value;
    let original_pid = backend_pid(connection)?;
    assert_ne!(
        original_pid, 0,
        "backend PID must identify an active session"
    );

    let duplicate =
        diesel::sql_query("INSERT INTO compat_diesel_probe (id, title) VALUES ($1, $2)")
            .bind::<Integer, _>(3_i32)
            .bind::<Text, _>("alpha")
            .execute(connection)
            .expect_err("duplicate unique value should fail");
    assert_unique_violation(&duplicate);
    let after_duplicate =
        diesel::sql_query("SELECT title AS value FROM compat_diesel_probe WHERE id = $1")
            .bind::<Integer, _>(1_i32)
            .get_result::<TextRow>(connection)?
            .value;
    assert_eq!(
        backend_pid(connection)?,
        original_pid,
        "session after duplicate"
    );

    let missing = diesel::sql_query("SELECT title FROM compat_diesel_missing")
        .execute(connection)
        .expect_err("missing relation should fail");
    let missing_message = database_error_message(&missing);
    assert!(
        missing_message.contains("does not exist"),
        "unexpected missing relation error: {missing_message}"
    );
    let after_missing =
        diesel::sql_query("SELECT title AS value FROM compat_diesel_probe WHERE id = $1")
            .bind::<Integer, _>(1_i32)
            .get_result::<TextRow>(connection)?
            .value;
    assert_eq!(
        backend_pid(connection)?,
        original_pid,
        "session after missing relation"
    );
    qualify_named_schema(connection, original_pid)?;

    println!("diesel_catalog={catalog}");
    println!("diesel_prepared={prepared}");
    println!("diesel_transaction_row_count={row_count}");
    println!("diesel_join={}:{}", joined.title, joined.status);
    println!("diesel_duplicate_error=unique_violation");
    println!("diesel_after_duplicate={after_duplicate}");
    println!("diesel_missing_error=relation_missing");
    println!("diesel_after_missing={after_missing}");
    println!("diesel_same_session=true");
    Ok(())
}

fn create_probe_data(connection: &mut PgConnection) -> Result<(), Error> {
    diesel::sql_query(
        "CREATE TABLE compat_diesel_probe (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE)",
    )
    .execute(connection)?;
    diesel::sql_query(
        "CREATE TABLE compat_diesel_status (id INT PRIMARY KEY, status TEXT NOT NULL)",
    )
    .execute(connection)?;

    connection.transaction::<_, Error, _>(|connection| {
        diesel::sql_query("INSERT INTO compat_diesel_probe (id, title) VALUES ($1, $2)")
            .bind::<Integer, _>(1_i32)
            .bind::<Text, _>("alpha")
            .execute(connection)?;
        diesel::sql_query("INSERT INTO compat_diesel_status (id, status) VALUES ($1, $2)")
            .bind::<Integer, _>(1_i32)
            .bind::<Text, _>("active")
            .execute(connection)?;
        Ok(())
    })
}

fn backend_pid(connection: &mut PgConnection) -> Result<i32, Error> {
    Ok(diesel::sql_query("SELECT pg_backend_pid() AS value")
        .get_result::<IntegerRow>(connection)?
        .value)
}

fn qualify_named_schema(connection: &mut PgConnection, original_pid: i32) -> Result<(), Error> {
    for sql in [
        "CREATE SCHEMA compat_diesel_namespace",
        "CREATE TABLE public.compat_diesel_shadow (id INT PRIMARY KEY, title TEXT NOT NULL)",
        "CREATE TABLE compat_diesel_namespace.compat_diesel_shadow \
         (id INT PRIMARY KEY, title TEXT NOT NULL)",
    ] {
        diesel::sql_query(sql).execute(connection)?;
    }
    for (sql, title) in [
        (
            "INSERT INTO public.compat_diesel_shadow VALUES ($1, $2)",
            "public",
        ),
        (
            "INSERT INTO compat_diesel_namespace.compat_diesel_shadow VALUES ($1, $2)",
            "named",
        ),
    ] {
        diesel::sql_query(sql)
            .bind::<Integer, _>(1_i32)
            .bind::<Text, _>(title)
            .execute(connection)?;
    }
    diesel::sql_query("SET search_path = compat_diesel_namespace, public").execute(connection)?;
    let named_schema = diesel::sql_query("SELECT current_schema() AS value")
        .get_result::<TextRow>(connection)?
        .value;
    let named_discovery = diesel::sql_query(
        "SELECT table_schema AS value FROM information_schema.tables \
         WHERE table_schema = $1 AND table_name = $2",
    )
    .bind::<Text, _>("compat_diesel_namespace")
    .bind::<Text, _>("compat_diesel_shadow")
    .get_result::<TextRow>(connection)?
    .value;
    let named_shadow =
        diesel::sql_query("SELECT title AS value FROM compat_diesel_shadow WHERE id = $1")
            .bind::<Integer, _>(1_i32)
            .get_result::<TextRow>(connection)?
            .value;
    diesel::sql_query("SET search_path = public").execute(connection)?;
    let restored_schema = diesel::sql_query("SELECT current_schema() AS value")
        .get_result::<TextRow>(connection)?
        .value;
    let public_shadow =
        diesel::sql_query("SELECT title AS value FROM compat_diesel_shadow WHERE id = $1")
            .bind::<Integer, _>(1_i32)
            .get_result::<TextRow>(connection)?
            .value;
    assert_eq!(named_schema, "compat_diesel_namespace");
    assert_eq!(named_discovery, "compat_diesel_namespace");
    assert_eq!(named_shadow, "named");
    assert_eq!(public_shadow, "public");
    assert_eq!(restored_schema, "public");
    assert_eq!(
        backend_pid(connection)?,
        original_pid,
        "schema workflow changed session"
    );
    println!("diesel_named_schema={named_schema}");
    println!("diesel_named_discovery={named_discovery}");
    println!("diesel_named_shadow={named_shadow}");
    println!("diesel_public_shadow={public_shadow}");
    println!("diesel_restored_schema={restored_schema}");
    println!("diesel_schema_same_session=true");
    Ok(())
}

fn assert_unique_violation(error: &Error) {
    let Error::DatabaseError(actual, _) = error else {
        panic!("expected database error, got {error}");
    };
    assert!(matches!(actual, DatabaseErrorKind::UniqueViolation));
}

fn database_error_message(error: &Error) -> &str {
    let Error::DatabaseError(_, information) = error else {
        panic!("expected database error, got {error}");
    };
    information.message()
}
