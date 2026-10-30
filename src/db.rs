// src/db.rs — 连接池 + 迁移（D-01 运行时查询 / D-25 sqlx::migrate!）
use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqliteSynchronous};
use sqlx::Sqlite;

pub fn db_path(data_dir: &str) -> String {
    Path::new(data_dir)
        .join("story-architect.db")
        .display()
        .to_string()
}

/// 每个连接统一 PRAGMA（§4.5）。
pub fn connect_options(data_dir: &str) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(db_path(data_dir))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_millis(5000))
}

pub async fn open_pool(data_dir: &str) -> Result<SqlitePool, sqlx::Error> {
    sqlx::pool::PoolOptions::<Sqlite>::new()
        .max_connections(4)
        .connect_with(connect_options(data_dir))
        .await
}

/// 编译期嵌入 migrations（不依赖运行目录）；失败拒绝启动。
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    let version = schema_version(pool).await?;
    // PRAGMA user_version 作为外部工具可读的版本镜像
    sqlx::query(&format!("PRAGMA user_version = {version}"))
        .execute(pool)
        .await?;
    Ok(())
}

/// 当前 schema 版本（D-03）。
pub async fn schema_version(pool: &SqlitePool) -> anyhow::Result<u64> {
    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;
    Ok(version.unwrap_or(0).max(0) as u64)
}

/// 启动备份：WAL checkpoint 后复制，保留 3 份（§4.3）。
pub async fn backup(pool: &SqlitePool, data_dir: &str) -> std::io::Result<()> {
    let path = db_path(data_dir);
    if !Path::new(&path).exists() {
        return Ok(());
    }
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await;
    let bak = format!("{path}.bak");
    let bak1 = format!("{path}.bak.1");
    let bak2 = format!("{path}.bak.2");
    let _ = std::fs::remove_file(&bak2);
    if Path::new(&bak1).exists() {
        let _ = std::fs::rename(&bak1, &bak2);
    }
    if Path::new(&bak).exists() {
        let _ = std::fs::rename(&bak, &bak1);
    }
    std::fs::copy(&path, &bak)?;
    Ok(())
}

/// 多行写事务统一入口（§4.5 `BEGIN IMMEDIATE`）。
pub async fn begin_immediate(
    pool: &SqlitePool,
) -> Result<sqlx::Transaction<'static, Sqlite>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}
