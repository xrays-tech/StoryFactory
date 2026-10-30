// src/jobs/session_recovery.rs — 启动恢复例程（§8.5）
use serde_json::json;
use sqlx::sqlite::SqlitePool;

use crate::error::AppError;
use crate::models::sandbox::SandboxEventType;
use crate::services::sandbox::events::persist_event;
use crate::services::util::db_err;

/// 进程重启后把所有活跃态会话标记 interrupted，并补发事件。
pub async fn run_startup(pool: &SqlitePool) -> Result<i64, AppError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT id, last_event_seq FROM sandbox_sessions WHERE status IN ('running','paused','waiting_input','finalizing')",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let count = rows.len() as i64;
    for (session_id, _last_seq) in rows {
        let mut tx = crate::db::begin_immediate(pool).await.map_err(db_err)?;
        sqlx::query(
            "UPDATE sandbox_sessions SET status = 'interrupted', error_code = 'PROCESS_RESTART', error_message = '进程重启，可恢复', updated_at = strftime('%s','now') WHERE id = ?",
        )
        .bind(&session_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        persist_event(
            &mut tx,
            &session_id,
            SandboxEventType::SessionInterrupted,
            json!({ "cause": "PROCESS_RESTART" }),
        )
        .await?;
        tx.commit().await.map_err(db_err)?;
    }
    Ok(count)
}
