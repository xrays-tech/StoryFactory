// tests/health.rs — 健康检查与中间件链（§5.7 / §12.1）
use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use story_architect::config;
use story_architect::router;
use story_architect::state::AppState;

async fn test_state() -> AppState {
    let dir = tempfile::tempdir().expect("tempdir");
    // tempdir 生命周期需覆盖 pool；改为泄漏（测试进程短命）
    let path = dir.keep();
    let data_dir = path.display().to_string();
    let pool = story_architect::db::open_pool(&data_dir)
        .await
        .expect("pool");
    story_architect::db::migrate(&pool).await.expect("migrate");
    let cfg = config::load().expect("config");
    let config = Arc::new(ArcSwap::from_pointee(cfg));
    let sessions = std::sync::Arc::new(story_architect::services::sandbox::SessionManager::new(
        pool.clone(),
        config.clone(),
        4,
    ));
    AppState {
        config,
        sessions,
        pool,
        bind_host: "127.0.0.1".to_string(),
        bind_port: 8080,
        is_public: false,
        app_token: String::new(),
        origins: vec!["http://127.0.0.1:8080".to_string()],
        dev_cors: false,
    }
}

#[tokio::test]
async fn healthz_ok() {
    let app = router::build_router(test_state().await);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/healthz")
                .header("host", "127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_ok_and_shape() {
    let app = router::build_router(test_state().await);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("host", "127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 16)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["status"], "ok");
    assert!(v["schema_version"].as_u64().unwrap_or(0) >= 1);
}

#[tokio::test]
async fn host_guard_rejects_foreign_host() {
    let app = router::build_router(test_state().await);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/healthz")
                .header("host", "evil.example:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn origin_guard_rejects_cross_origin_post() {
    let app = router::build_router(test_state().await);
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/healthz")
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn public_bind_requires_token() {
    let mut state = test_state().await;
    state.is_public = true;
    state.app_token = "abc123".to_string();
    let app = router::build_router(state.clone());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/healthz")
                .header("x-app-token", "abc123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
