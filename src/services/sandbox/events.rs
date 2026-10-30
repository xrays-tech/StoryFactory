// src/services/sandbox/events.rs — 事件定义与落盘/广播（§8.4 / §8.7）
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{Sqlite, Transaction};

use crate::models::sandbox::SandboxEventType;
use crate::services::util::db_err;

#[derive(Debug, Clone, Serialize)]
pub struct SandboxEvent {
    pub seq: u64,
    pub event_type: SandboxEventType,
    pub payload: Value,
    pub created_at: i64,
}

impl SandboxEvent {
    pub fn new(seq: u64, event_type: SandboxEventType, payload: Value) -> Self {
        Self {
            seq,
            event_type,
            payload,
            created_at: now_secs(),
        }
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 事件落盘 + 会话游标推进（同事务，唯一背压点）。
pub async fn persist_event(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
    event_type: SandboxEventType,
    payload: Value,
) -> Result<SandboxEvent, crate::error::AppError> {
    let last: Option<i64> =
        sqlx::query_scalar("SELECT last_event_seq FROM sandbox_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    let seq = last.unwrap_or(0) as u64 + 1;
    sqlx::query(
        "INSERT INTO sandbox_events(seq, session_id, event_type, payload) VALUES(?, ?, ?, ?)",
    )
    .bind(seq as i64)
    .bind(session_id)
    .bind(event_type.as_str())
    .bind(payload.to_string())
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    sqlx::query(
        "UPDATE sandbox_sessions SET last_event_seq = ?, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(seq as i64)
    .bind(session_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(SandboxEvent::new(seq, event_type, payload))
}

/// 便捷构造常见 payload。
pub fn payload(value: Value) -> Value {
    value
}

pub fn step_payload(step: u32, extra: Value) -> Value {
    let mut base = json!({ "step": step });
    if let (Some(obj), Some(extra_obj)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_obj {
            obj.insert(k.clone(), v.clone());
        }
    }
    base
}
