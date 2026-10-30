// src/jobs/novel_import_recovery.rs — 启动恢复：未完成任务标记失败
use sqlx::sqlite::SqlitePool;

use crate::error::AppError;
use crate::services::util::db_err;

/// 进程重启后，queued/analyzing 的内存 runner 已丢失，统一标记 failed。
pub async fn run_startup(pool: &SqlitePool) -> Result<i64, AppError> {
    let affected = sqlx::query(
        "UPDATE novel_imports SET status = 'failed', stage = 'failed', \
         error = '服务重启，任务中断，请重新上传', updated_at = strftime('%s','now') \
         WHERE status IN ('queued','analyzing')",
    )
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    Ok(affected as i64)
}
