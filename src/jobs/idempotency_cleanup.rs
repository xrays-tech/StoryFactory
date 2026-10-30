// src/jobs/idempotency_cleanup.rs — idempotency_records TTL 24h（§5.5）
use sqlx::sqlite::SqlitePool;

pub async fn run_once(pool: &SqlitePool) -> i64 {
    // 先清理卡死的 in-flight 占位（10 分钟），再执行 24h TTL。
    let _ = sqlx::query(
        "DELETE FROM idempotency_records WHERE status_code = 0 AND created_at < strftime('%s','now') - 600",
    )
    .execute(pool)
    .await;
    let result = sqlx::query(
        "DELETE FROM idempotency_records WHERE created_at < strftime('%s','now') - 86400",
    )
    .execute(pool)
    .await;
    match result {
        Ok(done) => done.rows_affected() as i64,
        Err(e) => {
            tracing::warn!(error = %e, "idempotency cleanup failed");
            0
        }
    }
}
