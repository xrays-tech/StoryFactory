// tests/common/mod.rs — P2 集成测试脚手架
#![allow(dead_code)]

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::Router;
use serde_json::Value;
use tower::ServiceExt;

use story_architect::config;
use story_architect::router;
use story_architect::state::AppState;

pub async fn test_state() -> AppState {
    test_state_with_chain(None).await
}

pub async fn test_state_with_chain(
    chain: Option<story_architect::llm::failover::ProviderChain>,
) -> AppState {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.keep();
    let data_dir = path.display().to_string();
    let pool = story_architect::db::open_pool(&data_dir)
        .await
        .expect("pool");
    story_architect::db::migrate(&pool).await.expect("migrate");
    let cfg = config::load().expect("config");
    let config = Arc::new(ArcSwap::from_pointee(cfg));
    let sessions = std::sync::Arc::new(match chain {
        Some(chain) => story_architect::services::sandbox::SessionManager::with_chain(
            pool.clone(),
            config.clone(),
            4,
            chain,
        ),
        None => {
            story_architect::services::sandbox::SessionManager::new(pool.clone(), config.clone(), 4)
        }
    });
    AppState {
        config: config.clone(),
        sessions,
        pool: pool.clone(),
        bind_host: "127.0.0.1".to_string(),
        bind_port: 8080,
        is_public: false,
        app_token: String::new(),
        origins: vec!["http://127.0.0.1:8080".to_string()],
        dev_cors: false,
        imports: std::sync::Arc::new(
            story_architect::services::novel_import::ImportRegistry::new(
                story_architect::services::novel_import::IMPORT_SEMAPHORE,
            ),
        ),
        drafts: std::sync::Arc::new(
            story_architect::services::chapter_draft::DraftRegistry::new(
                config.clone(),
                pool.clone(),
                None,
            ),
        ),
    }
}

pub fn test_app(state: AppState) -> Router {
    router::build_router(state)
}

pub async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _headers, value) = call_full(app, method, uri, body).await;
    (status, value)
}

pub async fn call_full(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080");
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let payload = body
        .map(|v| Body::from(v.to_string()))
        .unwrap_or_else(Body::empty);
    send(app, builder.body(payload).expect("request")).await
}

pub async fn call_raw(app: &Router, method: Method, uri: &str, raw: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("content-type", "application/json")
        .body(Body::from(raw.to_string()))
        .expect("request");
    let (status, _headers, value) = send(app, request).await;
    (status, value)
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, value)
}

pub async fn create_novel(app: &Router, title: &str) -> String {
    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/novels",
        Some(serde_json::json!({ "title": title })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create novel: {body}");
    body["id"].as_str().expect("novel id").to_string()
}

pub async fn create_character(app: &Router, novel_id: &str, name: &str) -> String {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/api/v1/novels/{novel_id}/characters"),
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create character: {body}");
    body["id"].as_str().expect("character id").to_string()
}

pub async fn create_volume(app: &Router, novel_id: &str, title: &str) -> String {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/api/v1/novels/{novel_id}/volumes"),
        Some(serde_json::json!({ "title": title })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create volume: {body}");
    body["id"].as_str().expect("volume id").to_string()
}

pub async fn create_chapter(app: &Router, novel_id: &str, volume_id: &str, title: &str) -> String {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/api/v1/novels/{novel_id}/volumes/{volume_id}/chapters"),
        Some(serde_json::json!({ "title": title, "scene_prompt": "场景" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create chapter: {body}");
    body["id"].as_str().expect("chapter id").to_string()
}

pub async fn call_with_headers(
    app: &Router,
    method: Method,
    uri: &str,
    headers: Vec<(&str, &str)>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080");
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    let payload = body
        .map(|v| Body::from(v.to_string()))
        .unwrap_or_else(Body::empty);
    send(app, builder.body(payload).expect("request")).await
}

pub async fn call_bytes(
    app: &Router,
    method: Method,
    uri: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("content-type", content_type)
        .body(Body::from(body))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("body")
        .to_vec();
    (status, headers, bytes)
}
