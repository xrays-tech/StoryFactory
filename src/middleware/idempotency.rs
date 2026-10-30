// src/middleware/idempotency.rs — 通用幂等（§5.5 / D-21）
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::models::common::validation_details;
use crate::services::util::db_err;
use crate::state::AppState;

const MAX_BODY: usize = 64 * 1024 * 1024;
const SNAPSHOT_LIMIT: usize = 16 * 1024;

fn scope_for(method: &Method, path: &str) -> Option<(&'static str, bool)> {
    if method != Method::POST {
        return None;
    }
    if path == "/api/v1/novels" {
        return Some(("novels", false));
    }
    if path.ends_with("/sandbox-sessions") {
        return Some(("sandbox_sessions", true));
    }
    if path == "/api/v1/import" {
        return Some(("import", false));
    }
    None
}

fn request_hash(method: &Method, path: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(method.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(path.as_bytes());
    hasher.update(b"\n");
    hasher.update(body);
    hex::encode(hasher.finalize())
}

fn replay_response(status: u16, body: String, replayed: bool) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if replayed {
        response.headers_mut().insert(
            header::HeaderName::from_static("idempotency-replayed"),
            HeaderValue::from_static("true"),
        );
    }
    response
}

pub async fn idempotency(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let Some((scope, forced)) = scope_for(&method, &path) else {
        return next.run(req).await;
    };

    let provided = req
        .headers()
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if forced && provided.is_none() {
        return AppError::validation(
            "缺少 Idempotency-Key",
            validation_details(vec![("Idempotency-Key", "该端点要求幂等键")]),
        )
        .into_response();
    }
    let key = provided.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if key.len() > 200 {
        return AppError::validation(
            "Idempotency-Key 过长",
            validation_details(vec![("Idempotency-Key", "最长 200 字符")]),
        )
        .into_response();
    }

    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, MAX_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return AppError::too_large().into_response();
        }
    };
    let hash = request_hash(&method, &path, &bytes);
    let req = Request::from_parts(parts, Body::from(bytes));

    // 占位插入 + 命中判定（SQLite 单写串行化）。
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO idempotency_records(key, scope, request_hash, status_code, response_body) VALUES(?, ?, ?, 0, '')",
    )
    .bind(&key)
    .bind(scope)
    .bind(&hash)
    .execute(&state.pool)
    .await;
    match inserted {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => {
            // 已有记录：等待在途请求完成（最多 2s）
            for _ in 0..100 {
                let row: Option<(String, i64, String)> = sqlx::query_as(
                    "SELECT request_hash, status_code, response_body FROM idempotency_records WHERE key = ? AND scope = ?",
                )
                .bind(&key)
                .bind(scope)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten();
                let Some((stored_hash, status, body)) = row else {
                    return AppError::internal("幂等记录读取失败").into_response();
                };
                if stored_hash != hash {
                    return AppError::new(
                        crate::error::ErrorCode::IdempotencyConflict,
                        "相同幂等键对应不同请求体",
                    )
                    .into_response();
                }
                if status != 0 {
                    return replay_response(status as u16, body, true);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            return AppError::new(
                crate::error::ErrorCode::IdempotencyConflict,
                "同一幂等请求仍在处理中",
            )
            .into_response();
        }
        Err(e) => return db_err(e).into_response(),
    }

    let response = next.run(req).await;
    let status = response.status();
    let (mut parts, body) = response.into_parts();
    let body_bytes = match to_bytes(body, MAX_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => return AppError::internal("响应体读取失败").into_response(),
    };

    if status.is_success() || status == StatusCode::CONFLICT {
        let (stored, hint) = if body_bytes.len() > SNAPSHOT_LIMIT {
            (
                format!(
                    "{{\"replay_hint\":true,\"status_code\":{}}}",
                    status.as_u16()
                ),
                1i64,
            )
        } else {
            (String::from_utf8_lossy(&body_bytes).to_string(), 0i64)
        };
        if let Err(e) = sqlx::query(
            "UPDATE idempotency_records SET status_code = ?, response_body = ?, replay_hint = ? WHERE key = ? AND scope = ?",
        )
        .bind(status.as_u16() as i64)
        .bind(&stored)
        .bind(hint)
        .bind(&key)
        .bind(scope)
        .execute(&state.pool)
        .await
        {
            tracing::warn!(error = %e, "幂等记录写入失败，删除占位以避免锁死");
            let _ = sqlx::query(
                "DELETE FROM idempotency_records WHERE key = ? AND scope = ? AND status_code = 0",
            )
            .bind(&key)
            .bind(scope)
            .execute(&state.pool)
            .await;
        }
    } else {
        sqlx::query(
            "DELETE FROM idempotency_records WHERE key = ? AND scope = ? AND status_code = 0",
        )
        .bind(&key)
        .bind(scope)
        .execute(&state.pool)
        .await
        .ok();
    }

    parts.headers.insert(
        header::HeaderName::from_static("idempotency-key"),
        HeaderValue::from_str(&key).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    Response::from_parts(parts, Body::from(body_bytes))
}
