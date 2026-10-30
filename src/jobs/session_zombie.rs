// src/jobs/session_zombie.rs — 僵尸会话扫描（§5.6）
use serde_json::json;
use sqlx::sqlite::SqlitePool;

use crate::error::AppError;
use crate::models::sandbox::SandboxEventType;
use crate::services::sandbox::events::persist_event;
use crate::services::util::db_err;

/// running/finalizing 10 分钟无事件、paused 超 session_deadline → interrupted。
pub async fn run_once(pool: &SqlitePool) -> Result<i64, AppError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, status FROM sandbox_sessions
         WHERE (status IN ('running','finalizing') AND updated_at < strftime('%s','now') - 600)
            OR (status IN ('paused','waiting_input') AND session_deadline IS NOT NULL AND session_deadline < strftime('%s','now'))",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let count = rows.len() as i64;
    for (session_id, _status) in rows {
        let mut tx = crate::db::begin_immediate(pool).await.map_err(db_err)?;
        sqlx::query(
            "UPDATE sandbox_sessions SET status = 'interrupted', error_code = 'ZOMBIE', error_message = '会话无活动，已标记可恢复', updated_at = strftime('%s','now') WHERE id = ?",
        )
        .bind(&session_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        persist_event(
            &mut tx,
            &session_id,
            SandboxEventType::SessionInterrupted,
            json!({ "cause": "ZOMBIE" }),
        )
        .await?;
        tx.commit().await.map_err(db_err)?;
    }
    Ok(count)
}
