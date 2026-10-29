use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error};
use diesel::sql_types::{BigInt, Integer, Text};

#[derive(QueryableByName)]
struct TextRow {
    #[diesel(sql_type = Text)]
    value: String,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    value: i64,
}

#[derive(QueryableByName)]
struct IntegerRow {
    #[diesel(sql_type = Integer)]
    value: i32,
}

#[derive(QueryableByName)]
struct JoinedRow {
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    status: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("expected PostgreSQL connection URL")?;
    let connection = &mut PgConnection::establish(&url)?;

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
    })?;

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

fn backend_pid(connection: &mut PgConnection) -> Result<i32, Error> {
    Ok(diesel::sql_query("SELECT pg_backend_pid() AS value")
        .get_result::<IntegerRow>(connection)?
        .value)
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
