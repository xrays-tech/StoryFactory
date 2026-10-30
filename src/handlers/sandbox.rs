// src/handlers/sandbox.rs — 沙盒会话端点（§6.2）
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::extract::{AppJson, AppQuery};
use crate::models::sandbox::{
    ControlAction, ControlReq, CreateSessionReq, InputReq, SessionStatus,
};
use crate::services::sandbox::manager::StartKind;
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionListQ {
    pub status: Option<SessionStatus>,
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
}

pub async fn create_chapter_session(
    state: State<AppState>,
    path: Path<String>,
    body: AppJson<CreateSessionReq>,
) -> Result<Response, AppError> {
    let cid = path.0;
    let novel_id: Option<String> =
        sqlx::query_scalar("SELECT novel_id FROM chapters WHERE id = ? AND deleted_at IS NULL")
            .bind(&cid)
            .fetch_optional(&state.pool)
            .await
            .map_err(crate::services::util::db_err)?;
    let novel_id = novel_id.ok_or_else(|| {
        AppError::new(
            crate::error::ErrorCode::ChapterNotFound,
            "章节不存在或已删除",
        )
    })?;
    let snapshot = state.sessions.create(&novel_id, Some(&cid), body.0).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/sandbox-sessions/{}", snapshot.id),
        snapshot,
    ))
}

pub async fn create_exploration_session(
    state: State<AppState>,
    path: Path<String>,
    body: AppJson<CreateSessionReq>,
) -> Result<Response, AppError> {
    let snapshot = state.sessions.create(&path.0, None, body.0).await?;
    Ok(crate::handlers::response::created(
        &format!("/api/v1/sandbox-sessions/{}", snapshot.id),
        snapshot,
    ))
}

pub async fn list_sessions(
    state: State<AppState>,
    path: Path<String>,
    query: AppQuery<SessionListQ>,
) -> Result<Response, AppError> {
    let page = state
        .sessions
        .list(&path.0, query.status, query.page, query.page_size)
        .await?;
    Ok((StatusCode::OK, Json(page)).into_response())
}

pub async fn get_session(state: State<AppState>, path: Path<String>) -> Result<Response, AppError> {
    let snapshot = state.sessions.snapshot(&path.0).await?;
    Ok((StatusCode::OK, Json(snapshot)).into_response())
}

pub async fn control_session(
    state: State<AppState>,
    path: Path<String>,
    body: AppJson<ControlReq>,
) -> Result<Response, AppError> {
    let sid = path.0;
    let action = body.0.action;
    match action {
        ControlAction::Start => {
            state.sessions.start_session(&sid, StartKind::Start).await?;
        }
        ControlAction::Resume => {
            state
                .sessions
                .start_session(&sid, StartKind::Resume)
                .await?;
        }
        ControlAction::Recover => {
            state
                .sessions
                .start_session(&sid, StartKind::Recover)
                .await?;
        }
        ControlAction::Step => {
            state.sessions.start_session(&sid, StartKind::Step).await?;
        }
        ControlAction::Pause | ControlAction::Abort => {
            state.sessions.control(&sid, action).await?;
        }
    }
    let snapshot = state.sessions.snapshot(&sid).await?;
    Ok((StatusCode::ACCEPTED, Json(snapshot)).into_response())
}

pub async fn input_session(
    state: State<AppState>,
    path: Path<String>,
    body: AppJson<InputReq>,
) -> Result<Response, AppError> {
    let snapshot = state.sessions.input(&path.0, &body.0.text).await?;
    Ok((StatusCode::OK, Json(snapshot)).into_response())
}
