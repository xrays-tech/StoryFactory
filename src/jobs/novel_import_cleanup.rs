// src/jobs/novel_import_cleanup.rs — 过期导入任务清理（保留终态任务 N 天）
use sqlx::sqlite::SqlitePool;

use crate::error::AppError;
use crate::services::util::db_err;

/// 删除超过保留期的终态导入任务，返回删除行数。
/// retention_days <= 0 时按 7 天处理。
pub async fn run_once(pool: &SqlitePool, retention_days: i64) -> Result<i64, AppError> {
    let days = if retention_days <= 0 {
        7
    } else {
        retention_days
    };
    let cutoff_secs = days.saturating_mul(86_400);
    let affected = sqlx::query(
        "DELETE FROM novel_imports WHERE status IN ('ready','failed','cancelled') \
         AND updated_at < strftime('%s','now') - ?",
    )
    .bind(cutoff_secs)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    Ok(affected as i64)
}
