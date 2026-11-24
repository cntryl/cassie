use sqlx::{postgres::PgPoolOptions, Connection};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("expected PostgreSQL connection URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;

    let mut connection = pool.acquire().await?;
    let backend_pid: i64 = sqlx::query_scalar("SELECT pg_backend_pid()::BIGINT")
        .fetch_one(&mut *connection)
        .await?;
    assert!(backend_pid > 0, "backend identity must be nonzero");

    let current_schema = prepare_named_schema(&mut connection).await?;

    let mut transaction = connection.begin().await?;
    sqlx::query("INSERT INTO compat_sqlx_probe (id, title) VALUES ($1, $2)")
        .bind(1_i32)
        .bind("alpha")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    let (catalog, named_catalog): (String, String) = sqlx::query_as(
        "SELECT table_name, table_schema FROM information_schema.tables \
         WHERE table_name = $1 AND table_schema = $2",
    )
    .bind("compat_sqlx_probe")
    .bind("compat_sqlx_named")
    .fetch_one(&mut *connection)
    .await?;
    let prepared = read_probe_title(&mut connection).await?;
    let named_shadow = read_probe_title(&mut connection).await?;

    let mut rolled_back = connection.begin().await?;
    sqlx::query("INSERT INTO compat_sqlx_probe (id, title) VALUES ($1, $2)")
        .bind(2_i32)
        .bind("beta")
        .execute(&mut *rolled_back)
        .await?;
    rolled_back.rollback().await?;
    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM compat_sqlx_probe")
        .fetch_one(&mut *connection)
        .await?;

    let joined: String = sqlx::query_scalar(
        "SELECT left_row.title FROM compat_sqlx_probe AS left_row \
         JOIN compat_sqlx_probe AS right_row ON left_row.id = right_row.id \
         WHERE right_row.id = $1",
    )
    .bind(1_i32)
    .fetch_one(&mut *connection)
    .await?;

    let duplicate = sqlx::query("INSERT INTO compat_sqlx_probe (id, title) VALUES ($1, $2)")
        .bind(3_i32)
        .bind("alpha")
        .execute(&mut *connection)
        .await
        .expect_err("duplicate unique value should fail");
    let after_duplicate = read_probe_title(&mut connection).await?;
    let backend_after_duplicate: i64 = sqlx::query_scalar("SELECT pg_backend_pid()::BIGINT")
        .fetch_one(&mut *connection)
        .await?;
    let missing = sqlx::query("SELECT title FROM compat_sqlx_missing")
        .execute(&mut *connection)
        .await
        .expect_err("missing relation should fail");
    let after_missing = read_probe_title(&mut connection).await?;
    let backend_after_missing: i64 = sqlx::query_scalar("SELECT pg_backend_pid()::BIGINT")
        .fetch_one(&mut *connection)
        .await?;
    assert_eq!(backend_pid, backend_after_duplicate);
    assert_eq!(backend_pid, backend_after_missing);
    let (restored_schema, public_shadow, backend_after_restore) =
        restore_public_schema(&mut connection).await?;
    assert_eq!(backend_pid, backend_after_restore);

    println!("sqlx_catalog={catalog}");
    println!("sqlx_prepared={prepared}");
    println!("sqlx_transaction_row_count={row_count}");
    println!("sqlx_duplicate_sqlstate={}", sqlstate(&duplicate));
    println!("sqlx_missing_sqlstate={}", sqlstate(&missing));
    println!("sqlx_joined={joined}");
    println!("sqlx_after_duplicate={after_duplicate}");
    println!("sqlx_after_missing={after_missing}");
    println!("sqlx_same_session=true");
    println!("sqlx_current_schema={current_schema}");
    println!("sqlx_named_catalog={named_catalog}");
    println!("sqlx_named_shadow={named_shadow}");
    println!("sqlx_public_shadow={public_shadow}");
    println!("sqlx_restored_schema={restored_schema}");
    Ok(())
}

async fn prepare_named_schema(connection: &mut sqlx::PgConnection) -> Result<String, sqlx::Error> {
    sqlx::query("CREATE SCHEMA compat_sqlx_named")
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "CREATE TABLE public.compat_sqlx_probe (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE)",
    )
    .execute(&mut *connection)
    .await?;
    sqlx::query("INSERT INTO public.compat_sqlx_probe (id, title) VALUES ($1, $2)")
        .bind(1_i32)
        .bind("public-shadow")
        .execute(&mut *connection)
        .await?;
    sqlx::query("SET search_path = compat_sqlx_named, public")
        .execute(&mut *connection)
        .await?;
    let current_schema: String = sqlx::query_scalar("SELECT current_schema()::TEXT")
        .fetch_one(&mut *connection)
        .await?;
    sqlx::query("CREATE TABLE compat_sqlx_probe (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE)")
        .execute(&mut *connection)
        .await?;
    Ok(current_schema)
}

async fn read_probe_title(connection: &mut sqlx::PgConnection) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT title FROM compat_sqlx_probe WHERE id = $1")
        .bind(1_i32)
        .fetch_one(connection)
        .await
}

async fn restore_public_schema(
    connection: &mut sqlx::PgConnection,
) -> Result<(String, String, i64), sqlx::Error> {
    sqlx::query("SET search_path = public")
        .execute(&mut *connection)
        .await?;
    let restored_schema: String = sqlx::query_scalar("SELECT current_schema()::TEXT")
        .fetch_one(&mut *connection)
        .await?;
    let public_shadow: String =
        sqlx::query_scalar("SELECT title FROM compat_sqlx_probe WHERE id = $1")
            .bind(1_i32)
            .fetch_one(&mut *connection)
            .await?;
    let backend_after_restore: i64 = sqlx::query_scalar("SELECT pg_backend_pid()::BIGINT")
        .fetch_one(&mut *connection)
        .await?;
    Ok((restored_schema, public_shadow, backend_after_restore))
}

fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .map_or_else(String::new, std::borrow::Cow::into_owned)
}
