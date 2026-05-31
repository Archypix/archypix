use sqlx::PgPool;
use tracing::info;

pub async fn init_database(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS backends (
            back_domain  VARCHAR(255) PRIMARY KEY,
            use_https    BOOLEAN NOT NULL,
            internal_url VARCHAR(255) NOT NULL,
            created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_mappings (
            username    VARCHAR(255) PRIMARY KEY,
            back_domain VARCHAR(255) NOT NULL REFERENCES backends(back_domain),
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_user_mappings_back_domain
        ON user_mappings(back_domain)
        "#,
    )
    .execute(pool)
    .await?;

    info!("Database schema initialized");
    Ok(())
}

/// Returns the full public backend URL for a user, built from the joined backend record.
pub async fn get_backend_url(pool: &PgPool, username: &str) -> anyhow::Result<Option<String>> {
    let result = sqlx::query_scalar::<_, String>(
        r#"
        SELECT CASE WHEN b.use_https THEN 'https://' ELSE 'http://' END || b.back_domain
        FROM user_mappings u
        JOIN backends b ON u.back_domain = b.back_domain
        WHERE u.username = $1
        "#,
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    Ok(result)
}

pub async fn upsert_mapping(
    pool: &PgPool,
    username: &str,
    back_domain: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO user_mappings (username, back_domain, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (username)
        DO UPDATE SET back_domain = $2, updated_at = NOW()
        "#,
    )
    .bind(username)
    .bind(back_domain)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn upsert_backend(
    pool: &PgPool,
    back_domain: &str,
    use_https: bool,
    internal_url: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO backends (back_domain, use_https, internal_url)
        VALUES ($1, $2, $3)
        ON CONFLICT (back_domain)
        DO UPDATE SET use_https = $2, internal_url = $3
        "#,
    )
    .bind(back_domain)
    .bind(use_https)
    .bind(internal_url)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_backends(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    let result =
        sqlx::query_scalar::<_, String>("SELECT back_domain FROM backends ORDER BY created_at ASC")
            .fetch_all(pool)
            .await?;

    Ok(result)
}

/// Returns `(back_domain, use_https, internal_url, user_count)`, ordered by user_count ASC.
pub async fn count_users_per_backend(
    pool: &PgPool,
) -> anyhow::Result<Vec<(String, bool, String, i64)>> {
    let rows = sqlx::query_as::<_, (String, bool, String, i64)>(
        r#"
        SELECT b.back_domain, b.use_https, b.internal_url, COUNT(u.username) AS user_count
        FROM backends b
        LEFT JOIN user_mappings u ON u.back_domain = b.back_domain
        GROUP BY b.back_domain, b.use_https, b.internal_url
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
