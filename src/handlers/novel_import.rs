// src/handlers/novel_import.rs — txt 小说上传/分析进度/预览/生成项目
use std::convert::Infallible;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::AppError;
use crate::extract::AppJson;
use crate::models::import::{CreateNovelFromImportReq, NovelImportJob, NovelImportStatus};
use crate::services::novel_import;
use crate::state::AppState;

const DEFAULT_FILENAME: &str = "novel.txt";

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/v1/novel-imports", post(create_import))
        .route(
            "/v1/novel-imports/:id",
            get(get_import).delete(delete_import),
        )
        .route("/v1/novel-imports/:id/preview", get(preview_import))
        .route("/v1/novel-imports/:id/events", get(stream_import))
        .route("/v1/novel-imports/:id/project", post(create_project))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// HTTP 头只允许 ISO-8859-1/obs-text；非 ASCII 字节逐字节映射为 U+00XX。
fn header_text(value: &HeaderValue) -> String {
    match value.to_str() {
        Ok(text) => text.to_string(),
        Err(_) => value.as_bytes().iter().map(|byte| *byte as char).collect(),
    }
}

/// 前端用 encodeURIComponent 传中文文件名。任何 %XX 损坏或解码后非 UTF-8 时，
/// 整体回退为原始头值，禁止把 `%E7%AC...` 原样写进 filename。
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(high) = bytes.get(index + 1).and_then(|byte| hex_value(*byte)) else {
                return raw.to_string();
            };
            let Some(low) = bytes.get(index + 2).and_then(|byte| hex_value(*byte)) else {
                return raw.to_string();
            };
            decoded.push(high * 16 + low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| raw.to_string())
}

fn filename_from(headers: &HeaderMap) -> String {
    headers
        .get("x-filename")
        .map(header_text)
        .map(|value| percent_decode(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_FILENAME.to_string())
}

pub async fn create_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let cfg = state.config.load().as_ref().clone();
    let job = novel_import::create_job(
        &state.pool,
        &filename_from(&headers),
        body.to_vec(),
        state.imports.clone(),
        cfg,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(job)).into_response())
}

pub async fn get_import(
    State(state): State<AppState>,
    path: Path<String>,
) -> Result<Response, AppError> {
    let job = novel_import::get_job(&state.pool, &path.0).await?;
    Ok(Json(job).into_response())
}

pub async fn preview_import(
    State(state): State<AppState>,
    path: Path<String>,
) -> Result<Response, AppError> {
    let preview = novel_import::get_preview(&state.pool, &path.0).await?;
    Ok(Json(preview).into_response())
}

pub async fn delete_import(
    State(state): State<AppState>,
    path: Path<String>,
) -> Result<Response, AppError> {
    novel_import::cancel_job(&state.pool, &path.0, &state.imports).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn create_project(
    State(state): State<AppState>,
    path: Path<String>,
    AppJson(req): AppJson<CreateNovelFromImportReq>,
) -> Result<Response, AppError> {
    let novel = novel_import::create_project(&state.pool, &path.0, req).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/novels/{}", novel.id),
        novel,
    ))
}

// ============================================================
// SSE：snapshot → progress* → done/failed
// ============================================================

fn terminal_event(status: NovelImportStatus) -> Option<&'static str> {
    match status {
        NovelImportStatus::Ready => Some("import.done"),
        NovelImportStatus::Failed | NovelImportStatus::Cancelled => Some("import.failed"),
        NovelImportStatus::Queued | NovelImportStatus::Analyzing => None,
    }
}

async fn send_job(
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    event: &str,
    job: &NovelImportJob,
) -> Result<(), ()> {
    let data = serde_json::to_string(job).map_err(|_| ())?;
    tx.send(Ok(Event::default().event(event).data(data)))
        .await
        .map_err(|_| ())
}

pub async fn stream_import(
    State(state): State<AppState>,
    path: Path<String>,
) -> Result<Response, AppError> {
    let id = path.0;
    let initial = novel_import::get_job(&state.pool, &id).await?;
    let heartbeat = state.config.load().http.sse_heartbeat_seconds.max(1) as u64;
    let pool = state.pool.clone();
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        if send_job(&tx, "import.snapshot", &initial).await.is_err() {
            return;
        }
        if let Some(event) = terminal_event(initial.status) {
            let _ = send_job(&tx, event, &initial).await;
            return;
        }

        let mut last = initial;
        let mut poll = tokio::time::interval(Duration::from_millis(700));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        poll.tick().await;
        let mut ping = tokio::time::interval(Duration::from_secs(heartbeat));
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping.tick().await;

        loop {
            tokio::select! {
                _ = poll.tick() => {
                    let current = match novel_import::load_job(&pool, &id).await {
                        Ok(Some(job)) => job,
                        _ => return,
                    };
                    if current != last {
                        if send_job(&tx, "import.progress", &current).await.is_err() {
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
                    if tx.send(Ok(Event::default().comment("ping"))).await.is_err() {
                        return;
                    }
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
