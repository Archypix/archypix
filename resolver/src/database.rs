use sqlx::PgPool;
use tracing::info;

pub async fn init_database(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_mappings (
            username VARCHAR(255) PRIMARY KEY,
            backend_url VARCHAR(255) NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_backend_url
        ON user_mappings(backend_url)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS backends (
            backend_url VARCHAR(255) PRIMARY KEY,
            name        VARCHAR(255) NOT NULL,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    info!("Database schema initialized");
    Ok(())
}

pub async fn get_backend_url(pool: &PgPool, username: &str) -> anyhow::Result<Option<String>> {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT backend_url FROM user_mappings WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    Ok(result)
}

pub async fn upsert_mapping(
    pool: &PgPool,
    username: &str,
    backend_url: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO user_mappings (username, backend_url, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (username)
        DO UPDATE SET backend_url = $2, updated_at = NOW()
        "#,
    )
    .bind(username)
    .bind(backend_url)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_backends(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    let result =
        sqlx::query_scalar::<_, String>("SELECT backend_url FROM backends ORDER BY created_at ASC")
            .fetch_all(pool)
            .await?;

    Ok(result)
}

pub async fn upsert_backend(pool: &PgPool, backend_url: &str, name: &str) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO backends (backend_url, name)
        VALUES ($1, $2)
        ON CONFLICT (backend_url)
        DO UPDATE SET name = $2
        "#,
    )
    .bind(backend_url)
    .bind(name)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn count_users_per_backend(pool: &PgPool) -> anyhow::Result<Vec<(String, String, i64)>> {
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        r#"
        SELECT b.backend_url, b.name, COUNT(u.username) as user_count
        FROM backends b
        LEFT JOIN user_mappings u ON u.backend_url = b.backend_url
        GROUP BY b.backend_url
        ORDER BY user_count ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn username_exists(pool: &PgPool, username: &str) -> anyhow::Result<bool> {
    let count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_mappings WHERE username = $1")
            .bind(username)
            .fetch_one(pool)
            .await?;

    Ok(count > 0)
}
