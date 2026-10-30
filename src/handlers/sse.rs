// src/handlers/sse.rs — SSE 事件流（§6.4 / §8.4 / D-20 / D-26）
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::AppError;
use crate::extract::AppQuery;
use crate::models::sandbox::{SandboxEventType, SessionStatus};
use crate::services::sandbox::manager::load_row;
use crate::services::util::db_err;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SseQ {
    pub after_seq: Option<u64>,
}

const IDLE_POLL_BASE_SECS: u64 = 2;
const IDLE_POLL_MAX_SECS: u64 = 15;

/// 无活跃引擎时的空闲轮询退避：2s → 4s → 8s → 15s 封顶。
fn idle_poll_delay(attempt: u32) -> Duration {
    let secs = IDLE_POLL_BASE_SECS.saturating_mul(1u64 << attempt.min(3));
    Duration::from_secs(secs.min(IDLE_POLL_MAX_SECS))
}

pub async fn stream_events(
    state: State<AppState>,
    path: Path<String>,
    query: AppQuery<SseQ>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session_id = path.0;
    let _ = load_row(&state.pool, &session_id).await?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let after = query.after_seq.unwrap_or(0).max(last_event_id);

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    let pool = state.pool.clone();
    let sessions = state.sessions.clone();
    let heartbeat = state.config.load().http.sse_heartbeat_seconds.max(1) as u64;

    tokio::spawn(async move {
        let mut last_sent = after;
        let mut idle_attempt = 0u32;
        let mut idle_observed: Option<(String, i64)> = None;
        // D-20：先订阅再回放，避免订阅空窗丢事件。
        let mut live = sessions.subscribe(&session_id);
        match replay(&pool, &session_id, after, &tx, &mut last_sent).await {
            Ok(true) => {
                let _ = send_done(&tx).await;
                return;
            }
            Ok(false) => {}
            Err(_) => return,
        }

        loop {
            if let Some(receiver) = live.as_mut() {
                let mut closed = false;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(heartbeat)) => {
                        if tx.send(Ok(Event::default().comment("ping"))).await.is_err() {
                            return;
                        }
                    }
                    received = receiver.recv() => {
                        match received {
                            Ok(event) => {
                                if event.seq <= last_sent {
                                    continue;
                                }
                                let terminal = is_terminal(event.event_type);
                                if send_event(&tx, &event).await.is_err() {
                                    return;
                                }
                                last_sent = event.seq;
                                if terminal {
                                    let _ = send_done(&tx).await;
                                    return;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                let since = last_sent;
                                match replay(&pool, &session_id, since, &tx, &mut last_sent).await {
                                    Ok(true) => {
                                        let _ = send_done(&tx).await;
                                        return;
                                    }
                                    Ok(false) => {}
                                    Err(_) => return,
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                closed = true;
                            }
                        }
                    }
                }
                if closed {
                    live = None;
                    idle_attempt = 0;
                    idle_observed = None;
                }
            } else {
                // 无活跃任务：指数退避轮询终态，并持续尝试重新订阅（resume/recover 会产生新 hub）。
                let delay = idle_poll_delay(idle_attempt);
                idle_attempt = idle_attempt.saturating_add(1);
                tokio::time::sleep(delay).await;
                match load_row(&pool, &session_id).await {
                    Ok(row) => {
                        let observed = (row.status.clone(), row.current_step);
                        let changed = idle_observed
                            .as_ref()
                            .is_some_and(|previous| previous != &observed);
                        idle_observed = Some(observed);
                        if changed {
                            // 状态/进度有变化：回到 2s 快路径重新观察。
                            idle_attempt = 0;
                        }
                        if let Some(status) = SessionStatus::from_db(&row.status) {
                            if status.is_terminal() {
                                let _ = send_done(&tx).await;
                                return;
                            }
                        }
                    }
                    Err(_) => return,
                }
                if let Some(receiver) = sessions.subscribe(&session_id) {
                    idle_attempt = 0;
                    idle_observed = None;
                    let since = last_sent;
                    match replay(&pool, &session_id, since, &tx, &mut last_sent).await {
                        Ok(true) => {
                            let _ = send_done(&tx).await;
                            return;
                        }
                        Ok(false) => {}
                        Err(_) => return,
                    }
                    live = Some(receiver);
                }
                if tx.send(Ok(Event::default().comment("ping"))).await.is_err() {
                    return;
                }
            }
        }
    });

    let mut response = Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(heartbeat)))
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-accel-buffering"),
        header::HeaderValue::from_static("no"),
    );
    Ok(response)
}

async fn replay(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    after: u64,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    last_sent: &mut u64,
) -> Result<bool, AppError> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT seq, event_type, payload FROM sandbox_events WHERE session_id = ? AND seq > ? ORDER BY seq",
    )
    .bind(session_id)
    .bind(after as i64)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut terminal = false;
    for (seq, event_type, payload) in rows {
        let value: serde_json::Value =
            serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null);
        let event = Event::default()
            .id(seq.to_string())
            .event(event_type.clone())
            .data(value.to_string());
        if tx.send(Ok(event)).await.is_err() {
            return Ok(true);
        }
        *last_sent = seq as u64;
        if matches!(
            event_type.as_str(),
            "session_completed" | "session_failed" | "session_aborted"
        ) {
            terminal = true;
        }
    }
    Ok(terminal)
}

async fn send_event(
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    event: &crate::services::sandbox::events::SandboxEvent,
) -> Result<(), ()> {
    let payload = Event::default()
        .id(event.seq.to_string())
        .event(event.event_type.as_str())
        .data(event.payload.to_string());
    tx.send(Ok(payload)).await.map_err(|_| ())
}

async fn send_done(tx: &mpsc::Sender<Result<Event, Infallible>>) -> Result<(), ()> {
    tx.send(Ok(Event::default()
        .event("sandbox.done")
        .data("{\"reason\":\"terminal\"}")))
        .await
        .map_err(|_| ())
}

fn is_terminal(event_type: SandboxEventType) -> bool {
    matches!(
        event_type,
        SandboxEventType::SessionCompleted
            | SandboxEventType::SessionFailed
            | SandboxEventType::SessionAborted
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::idle_poll_delay;

    #[test]
    fn idle_poll_delay_backs_off_and_caps() {
        let secs: Vec<u64> = (0..=5)
            .map(|attempt| idle_poll_delay(attempt).as_secs())
            .collect();
        assert_eq!(secs, vec![2, 4, 8, 15, 15, 15]);
        assert_eq!(idle_poll_delay(0), Duration::from_secs(2));
    }
}
