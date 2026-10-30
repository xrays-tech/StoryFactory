// tests/p6.rs — SSE 线协议与幂等（§5.5 / §6.4）
mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::json;
use story_architect::llm::failover::ProviderChain;
use story_architect::llm::mock::MockProvider;
use story_architect::llm::StructuredMode;
use story_architect::models::sandbox::{CreateSessionReq, DirectorMode, SessionStatus};
use tower::ServiceExt;

use common::{
    create_chapter, create_character, create_novel, create_volume, test_app, test_state_with_chain,
};

fn actor_json(text: &str) -> String {
    json!({
        "actor_action": { "type": "dialogue", "content": text },
        "emotion_delta": { "target_actor_id": "self", "affinity_delta": 0, "trust_delta": 0 }
    })
    .to_string()
}

fn mock_chain(responses: Vec<String>) -> ProviderChain {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    for response in responses {
        provider.push_ok(response);
    }
    ProviderChain::new(vec![provider], 0, Duration::from_secs(2))
}

async fn completed_session(state: &story_architect::state::AppState) -> (String, String) {
    let app = test_app(state.clone());
    let nid = create_novel(&app, "SSE 测试").await;
    let a = create_character(&app, &nid, "角色A").await;
    let vid = create_volume(&app, &nid, "卷一").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;
    let req = CreateSessionReq {
        actor_ids: vec![a],
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    for _ in 0..300 {
        let snap = state.sessions.snapshot(&created.id).await.unwrap();
        if snap.status == SessionStatus::Completed {
            return (created.id, cid);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("session did not complete");
}

#[tokio::test]
async fn sse_replays_events_and_done() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("SSE 行动"),
        "## 章节标题\nSSE 大纲".to_string(),
    ])))
    .await;
    let (sid, _cid) = completed_session(&state).await;
    let app = test_app(state.clone());
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/v1/sandbox-sessions/{sid}/events?after_seq=0"))
        .header("host", "127.0.0.1:8080")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-cache")
    );
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("event: session_started"), "{text}");
    assert!(text.contains("event: actor_action"), "{text}");
    assert!(text.contains("event: session_completed"), "{text}");
    assert!(text.contains("event: sandbox.done"), "{text}");
}

#[tokio::test]
async fn sse_after_seq_skips_earlier_events() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("A"),
        actor_json("B"),
        "## 章节标题\n大纲".to_string(),
    ])))
    .await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "SSE after_seq").await;
    let a = create_character(&app, &nid, "角色A").await;
    let vid = create_volume(&app, &nid, "卷一").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;
    let req = CreateSessionReq {
        actor_ids: vec![a],
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(2),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    for _ in 0..300 {
        if state.sessions.snapshot(&created.id).await.unwrap().status == SessionStatus::Completed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let last_seq: i64 =
        sqlx::query_scalar("SELECT MAX(seq) FROM sandbox_events WHERE session_id = ?")
            .bind(&created.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/api/v1/sandbox-sessions/{}/events?after_seq={}",
            created.id,
            last_seq - 1
        ))
        .header("host", "127.0.0.1:8080")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(!text.contains("event: session_started"), "{text}");
    assert!(text.contains("event: sandbox.done"), "{text}");
}

#[tokio::test]
async fn session_create_is_idempotent_and_requires_key() {
    let state = test_state_with_chain(Some(mock_chain(vec![]))).await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "幂等测试").await;
    let a = create_character(&app, &nid, "角色A").await;
    let vid = create_volume(&app, &nid, "卷一").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;
    let body = json!({
        "actor_ids": [a],
        "mode": "roundrobin",
        "scene_prompt": "雨夜的旧城车站，两人重逢。",
        "step_limit": 2,
        "autostart": false
    });
    let uri = format!("/api/v1/chapters/{cid}/sandbox-sessions");

    let (status, _headers, _body) =
        common::call_with_headers(&app, Method::POST, &uri, vec![], Some(body.clone())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, headers, first) = common::call_with_headers(
        &app,
        Method::POST,
        &uri,
        vec![("idempotency-key", "k1")],
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let sid = first["id"].as_str().unwrap().to_string();

    let (status, headers2, second) = common::call_with_headers(
        &app,
        Method::POST,
        &uri,
        vec![("idempotency-key", "k1")],
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_eq!(second["id"], sid);
    assert_eq!(
        headers2
            .get("idempotency-replayed")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );

    let (status, _headers, conflict) = common::call_with_headers(
        &app,
        Method::POST,
        &uri,
        vec![("idempotency-key", "k1")],
        Some(json!({
            "actor_ids": [a],
            "mode": "roundrobin",
            "scene_prompt": "完全不同的场景描述文本。",
            "step_limit": 1,
            "autostart": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sandbox_sessions WHERE novel_id = ?")
        .bind(&nid)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let _ = headers;
}

#[tokio::test]
async fn sse_attaches_to_hub_created_after_connect() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_delay_ok(Duration::from_secs(3), actor_json("延迟行动"));
    provider.push_ok("## 章节标题\n延迟大纲".to_string());
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider],
        0,
        Duration::from_secs(5),
    )))
    .await;
    {
        let mut cfg = state.config.load().as_ref().clone();
        cfg.http.sse_heartbeat_seconds = 1;
        state.config.store(cfg.into());
    }
    let app = test_app(state.clone());
    let nid = create_novel(&app, "SSE 重订阅").await;
    let a = create_character(&app, &nid, "角色A").await;
    let vid = create_volume(&app, &nid, "卷一").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;
    let req = CreateSessionReq {
        actor_ids: vec![a],
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/api/v1/sandbox-sessions/{}/events?after_seq=0",
            created.id
        ))
        .header("host", "127.0.0.1:8080")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let collector = tokio::spawn(async move {
        axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    state
        .sessions
        .start_session(
            &created.id,
            story_architect::services::sandbox::manager::StartKind::Start,
        )
        .await
        .unwrap();
    let body = tokio::time::timeout(Duration::from_secs(10), collector)
        .await
        .expect("sse timeout")
        .expect("collector");
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("event: session_started"), "{text}");
    assert!(text.contains("event: actor_action"), "{text}");
    assert!(text.contains("event: sandbox.done"), "{text}");
}
