// src/handlers/chapter_draft.rs — 章节正文生成草稿端点（创建 / 查询 / SSE / 取消 / 保存正文）
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::AppError;
use crate::extract::AppJson;
use crate::models::draft::{CreateDraftReq, DraftJob, DraftStatus, SaveChapterContentReq};
use crate::services::chapter_draft;
use crate::state::AppState;

const POLL_INTERVAL: Duration = Duration::from_millis(700);
const PING_INTERVAL: Duration = Duration::from_secs(15);

pub fn router() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/v1/novels/:nid/chapters/:cid/drafts", post(create_draft))
        .route(
            "/v1/chapter-drafts/:id",
            get(get_draft).delete(delete_draft),
        )
        .route("/v1/chapter-drafts/:id/events", get(stream_draft))
        .route(
            "/v1/novels/:nid/chapters/:cid/content",
            get(get_chapter_content).put(save_chapter_content),
        )
}

async fn create_draft(
    State(state): State<AppState>,
    Path((novel_id, chapter_id)): Path<(String, String)>,
    AppJson(req): AppJson<CreateDraftReq>,
) -> Result<Response, AppError> {
    let job = state.drafts.create(&novel_id, &chapter_id, req).await?;
    Ok((StatusCode::ACCEPTED, Json(job)).into_response())
}

async fn get_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let job = chapter_draft::get_job(&state.pool, &id).await?;
    Ok(Json(job).into_response())
}

async fn get_chapter_content(
    State(state): State<AppState>,
    Path((novel_id, chapter_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let content = chapter_draft::load_content(&state.pool, &novel_id, &chapter_id).await?;
    Ok(Json(content).into_response())
}

async fn save_chapter_content(
    State(state): State<AppState>,
    Path((novel_id, chapter_id)): Path<(String, String)>,
    AppJson(req): AppJson<SaveChapterContentReq>,
) -> Result<Response, AppError> {
    chapter_draft::save_content(&state.pool, &novel_id, &chapter_id, &req.content).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn delete_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    chapter_draft::cancel_job(&state.pool, &id, &state.drafts).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ============================================================
// SSE：snapshot → progress* → done/failed
// ============================================================

fn terminal_event(status: DraftStatus) -> Option<&'static str> {
    match status {
        DraftStatus::Ready => Some("draft.done"),
        DraftStatus::Failed | DraftStatus::Cancelled => Some("draft.failed"),
        DraftStatus::Queued | DraftStatus::Generating => None,
    }
}

async fn send_job(
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    event: &str,
    job: &DraftJob,
) -> Result<(), ()> {
    let data = serde_json::to_string(job).map_err(|_| ())?;
    tx.send(Ok(Event::default().event(event).data(data)))
        .await
        .map_err(|_| ())
}

async fn stream_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let initial = chapter_draft::get_job(&state.pool, &id).await?;
    let pool = state.pool.clone();
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        if send_job(&tx, "draft.snapshot", &initial).await.is_err() {
            return;
        }
        if let Some(event) = terminal_event(initial.status) {
            let _ = send_job(&tx, event, &initial).await;
            return;
        }

        let mut last = initial;
        let mut poll = tokio::time::interval(POLL_INTERVAL);
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        poll.tick().await;
        let mut ping = tokio::time::interval(PING_INTERVAL);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping.tick().await;

        loop {
            tokio::select! {
                _ = poll.tick() => {
                    let current = match chapter_draft::load_job(&pool, &id).await {
                        Ok(Some(job)) => job,
                        _ => return,
                    };
                    if current != last {
                        if send_job(&tx, "draft.progress", &current).await.is_err() {
                            return;
                        }
                        last = current.clone();
                    }
                    if let Some(event) = terminal_event(current.status) {
                        let _ = send_job(&tx, event, &current).await;
                        return;
                    }
                }
                _ = ping.tick() => {
                    if tx
                        .send(Ok(Event::default().comment("ping")))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    });

    let mut response = Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(PING_INTERVAL))
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
