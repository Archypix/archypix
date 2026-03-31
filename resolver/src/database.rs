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

pub async fn upsert_mapping(pool: &PgPool, username: &str, backend_url: &str) -> anyhow::Result<()> {
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