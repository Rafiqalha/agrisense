use sqlx::{postgres::PgPoolOptions, PgPool};

pub type DbPool = PgPool;

pub async fn create_pool(database_url: &str, max_connections: u32) -> anyhow::Result<DbPool> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> anyhow::Result<()> {
    sqlx::migrate!("../../infrastructure/postgres/migrations")
        .run(pool)
        .await?;
    Ok(())
}

pub async fn is_ready(pool: &DbPool) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok()
}
