use sqlx::postgres::PgPoolOptions;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("expected PostgreSQL connection URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;

    sqlx::query("CREATE TABLE compat_sqlx_probe (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE)")
        .execute(&pool)
        .await?;

    let mut transaction = pool.begin().await?;
    sqlx::query("INSERT INTO compat_sqlx_probe (id, title) VALUES ($1, $2)")
        .bind(1_i32)
        .bind("alpha")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    let catalog: String = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables WHERE table_name = $1",
    )
    .bind("compat_sqlx_probe")
    .fetch_one(&pool)
    .await?;
    let prepared: String = sqlx::query_scalar("SELECT title FROM compat_sqlx_probe WHERE id = $1")
        .bind(1_i32)
        .fetch_one(&pool)
        .await?;

    let mut rolled_back = pool.begin().await?;
    sqlx::query("INSERT INTO compat_sqlx_probe (id, title) VALUES ($1, $2)")
        .bind(2_i32)
        .bind("beta")
        .execute(&mut *rolled_back)
        .await?;
    rolled_back.rollback().await?;
    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM compat_sqlx_probe")
        .fetch_one(&pool)
        .await?;

    let mut connection = pool.acquire().await?;
    let backend_pid: i64 = sqlx::query_scalar("SELECT pg_backend_pid()::BIGINT")
        .fetch_one(&mut *connection)
        .await?;
    assert!(backend_pid > 0, "backend identity must be nonzero");
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
    let after_duplicate: String =
        sqlx::query_scalar("SELECT title FROM compat_sqlx_probe WHERE id = $1")
            .bind(1_i32)
            .fetch_one(&mut *connection)
            .await?;
    let backend_after_duplicate: i64 = sqlx::query_scalar("SELECT pg_backend_pid()::BIGINT")
        .fetch_one(&mut *connection)
        .await?;
    let missing = sqlx::query("SELECT title FROM compat_sqlx_missing")
        .execute(&mut *connection)
        .await
        .expect_err("missing relation should fail");
    let after_missing: String =
        sqlx::query_scalar("SELECT title FROM compat_sqlx_probe WHERE id = $1")
            .bind(1_i32)
            .fetch_one(&mut *connection)
            .await?;
    let backend_after_missing: i64 = sqlx::query_scalar("SELECT pg_backend_pid()::BIGINT")
        .fetch_one(&mut *connection)
        .await?;
    assert_eq!(backend_pid, backend_after_duplicate);
    assert_eq!(backend_pid, backend_after_missing);

    println!("sqlx_catalog={catalog}");
    println!("sqlx_prepared={prepared}");
    println!("sqlx_transaction_row_count={row_count}");
    println!("sqlx_duplicate_sqlstate={}", sqlstate(&duplicate));
    println!("sqlx_missing_sqlstate={}", sqlstate(&missing));
    println!("sqlx_joined={joined}");
    println!("sqlx_after_duplicate={after_duplicate}");
    println!("sqlx_after_missing={after_missing}");
    println!("sqlx_same_session=true");
    Ok(())
}

fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .map_or_else(String::new, std::borrow::Cow::into_owned)
}
