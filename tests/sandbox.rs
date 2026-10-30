// tests/sandbox.rs — P5 引擎集成测试（mock LLM，无网络）
mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use story_architect::llm::failover::ProviderChain;
use story_architect::llm::mock::MockProvider;
use story_architect::llm::StructuredMode;
use story_architect::models::sandbox::{
    ControlAction, CreateSessionReq, DirectorMode, SessionSnapshot, SessionStatus,
};
use story_architect::state::AppState;

use common::{
    create_chapter, create_character, create_novel, create_volume, test_app, test_state_with_chain,
};

fn actor_json(text: &str) -> String {
    json!({
        "actor_action": { "type": "dialogue", "content": text },
        "emotion_delta": { "target_actor_id": "self", "affinity_delta": 0, "trust_delta": 0 },
        "director_note": "继续"
    })
    .to_string()
}

fn outline_text() -> String {
    "## 章节标题\n雨夜\n## 剧情梗概\n这是测试大纲。".to_string()
}

fn mock_chain(responses: Vec<String>) -> ProviderChain {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    for response in responses {
        provider.push_ok(response);
    }
    ProviderChain::new(vec![provider], 0, Duration::from_secs(2))
}

async fn base_novel(state: &AppState) -> (String, String, String) {
    let app = test_app(state.clone());
    let nid = create_novel(&app, "沙盒测试小说").await;
    let a = create_character(&app, &nid, "角色A").await;
    let b = create_character(&app, &nid, "角色B").await;
    let vid = create_volume(&app, &nid, "第一卷").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;
    (nid, format!("{a},{b}"), cid)
}

async fn wait_for<F>(state: &AppState, sid: &str, predicate: F) -> SessionSnapshot
where
    F: Fn(&SessionSnapshot) -> bool,
{
    for _ in 0..300 {
        let snapshot = state.sessions.snapshot(sid).await.expect("snapshot");
        if predicate(&snapshot) {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let snapshot = state.sessions.snapshot(sid).await.expect("snapshot");
    panic!("timeout waiting for session state: {:?}", snapshot.status);
}

#[tokio::test]
async fn roundrobin_runs_to_completion_and_writes_outline() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("A 的行动"),
        actor_json("B 的行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let actor_ids: Vec<String> = actors.split(',').map(str::to_string).collect();
    let req = CreateSessionReq {
        actor_ids,
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(2),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state
        .sessions
        .create(&nid, Some(&cid), req)
        .await
        .expect("create");
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.current_step, 2);
    let outline: Option<String> =
        sqlx::query_scalar("SELECT final_outline FROM chapters WHERE id = ?")
            .bind(&cid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(outline.unwrap_or_default().contains("章节标题"));
    let events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sandbox_events WHERE session_id = ?")
            .bind(&created.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(events >= 5, "events={events}");
}

#[tokio::test]
async fn director_mode_uses_llm_decision() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        json!({ "next_actor_id": "placeholder", "reason": "冲突" }).to_string(),
        actor_json("被选中的行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let actor_ids: Vec<String> = actors.split(',').map(str::to_string).collect();
    let first = actor_ids[0].clone();
    // 重新构造 chain，使导演选择第一个角色
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_ok(json!({ "next_actor_id": first, "reason": "冲突" }).to_string());
    provider.push_ok(actor_json("被选中的行动"));
    provider.push_ok(outline_text());
    let state2 = {
        let mut state = state;
        state.sessions = Arc::new(
            story_architect::services::sandbox::SessionManager::with_chain(
                state.pool.clone(),
                state.config.clone(),
                4,
                ProviderChain::new(vec![provider], 0, Duration::from_secs(2)),
            ),
        );
        state
    };
    let req = CreateSessionReq {
        actor_ids,
        mode: DirectorMode::Director,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state2.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let done = wait_for(&state2, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.current_step, 1);
    let payload: String = sqlx::query_scalar(
        "SELECT payload FROM sandbox_events WHERE session_id = ? AND event_type = 'actor_action' LIMIT 1",
    )
    .bind(&created.id)
    .fetch_one(&state2.pool)
    .await
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(value["actor_id"], first);
}

#[tokio::test]
async fn step_executes_one_step_and_stays_paused() {
    let state = test_state_with_chain(Some(mock_chain(vec![actor_json("单步行动")]))).await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(5),
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    sqlx::query("UPDATE sandbox_sessions SET status = 'paused' WHERE id = ?")
        .bind(&created.id)
        .execute(&state.pool)
        .await
        .unwrap();
    state
        .sessions
        .start_session(
            &created.id,
            story_architect::services::sandbox::manager::StartKind::Step,
        )
        .await
        .unwrap();
    let after = wait_for(&state, &created.id, |s| s.current_step >= 1).await;
    assert_eq!(after.status, SessionStatus::Paused);
    assert_eq!(after.current_step, 1);
}

#[tokio::test]
async fn abort_during_llm_drops_half_step() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_delay_ok(Duration::from_secs(2), actor_json("慢响应"));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(5),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    state
        .sessions
        .control(&created.id, ControlAction::Abort)
        .await
        .unwrap();
    let after = wait_for(&state, &created.id, |s| s.status == SessionStatus::Aborted).await;
    assert_eq!(after.current_step, 0);
    let actions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'actor_action'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(actions, 0, "half step must not persist");
}

#[tokio::test]
async fn recover_resumes_interrupted_session() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("恢复行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    sqlx::query("UPDATE sandbox_sessions SET status = 'interrupted', error_code = 'PROCESS_RESTART' WHERE id = ?")
        .bind(&created.id)
        .execute(&state.pool)
        .await
        .unwrap();
    state
        .sessions
        .start_session(
            &created.id,
            story_architect::services::sandbox::manager::StartKind::Recover,
        )
        .await
        .unwrap();
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.current_step, 1);
}

#[tokio::test]
async fn waiting_input_then_input_continues() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let (_nid0, _actors0, _cid0) = {
        let state = test_state_with_chain(Some(ProviderChain::new(
            vec![provider.clone()],
            0,
            Duration::from_secs(2),
        )))
        .await;
        let (nid, actors, cid) = base_novel(&state).await;
        (nid, actors, cid)
    };
    // 重新建立 state（base_novel 使用的 state 已离开作用域）
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "沙盒测试小说2").await;
    let a = create_character(&app, &nid, "角色A").await;
    let vid = create_volume(&app, &nid, "第一卷").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;
    provider.push_ok(json!({ "next_actor_id": a, "reason": "等待选择", "request_user_input": { "question": "走向？", "suggestions": ["A","B"] } }).to_string());
    provider
        .push_ok(json!({ "next_actor_id": a, "reason": "收束", "terminate": true }).to_string());
    provider.push_ok(outline_text());
    let req = CreateSessionReq {
        actor_ids: vec![a],
        mode: DirectorMode::Director,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(2),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let waiting = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::WaitingInput
    })
    .await;
    assert_eq!(waiting.status, SessionStatus::WaitingInput);
    state.sessions.input(&created.id, "选择 A").await.unwrap();
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.status, SessionStatus::Completed);
    let input_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'user_input_received'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(input_events, 1);
}

#[tokio::test]
async fn startup_recovery_marks_active_sessions_interrupted() {
    let state = test_state_with_chain(Some(mock_chain(vec![actor_json("x")]))).await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(3),
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    sqlx::query("UPDATE sandbox_sessions SET status = 'running' WHERE id = ?")
        .bind(&created.id)
        .execute(&state.pool)
        .await
        .unwrap();
    let recovered = story_architect::jobs::session_recovery::run_startup(&state.pool)
        .await
        .unwrap();
    assert!(recovered >= 1);
    let snap = state.sessions.snapshot(&created.id).await.unwrap();
    assert_eq!(snap.status, SessionStatus::Interrupted);
    assert!(snap.can_recover);
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'session_interrupted'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(events, 1);
}

#[tokio::test]
async fn http_create_get_and_list_sessions() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("路由行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let app = test_app(state.clone());
    let body = json!({
        "actor_ids": actors.split(',').collect::<Vec<_>>(),
        "mode": "roundrobin",
        "scene_prompt": "雨夜的旧城车站，两人重逢。",
        "step_limit": 1,
        "autostart": true
    });
    let (status, _headers, created) = common::call_with_headers(
        &app,
        axum::http::Method::POST,
        &format!("/api/v1/chapters/{cid}/sandbox-sessions"),
        vec![("idempotency-key", "http-create-1")],
        Some(body),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CREATED, "{created}");
    let sid = created["id"].as_str().unwrap().to_string();
    let (status, got) = common::call(
        &app,
        axum::http::Method::GET,
        &format!("/api/v1/sandbox-sessions/{sid}"),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(got["novel_id"], nid);
    let (status, list) = common::call(
        &app,
        axum::http::Method::GET,
        &format!("/api/v1/novels/{nid}/sandbox-sessions"),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(list["total"], 1);
}

#[tokio::test]
async fn concurrent_start_allows_only_one_engine() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("并发行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(2),
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let kind = story_architect::services::sandbox::manager::StartKind::Start;
    let (a, b) = tokio::join!(
        state.sessions.start_session(&created.id, kind),
        state.sessions.start_session(&created.id, kind)
    );
    let ok_count = [a.is_ok(), b.is_ok()].iter().filter(|v| **v).count();
    assert_eq!(ok_count, 1, "exactly one start must win");
    state
        .sessions
        .control(&created.id, ControlAction::Abort)
        .await
        .ok();
}

#[tokio::test]
async fn waiting_input_timeout_finalizes() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        json!({ "next_actor_id": "x", "reason": "等待", "request_user_input": { "question": "选择？" } }).to_string(),
        outline_text(),
    ])))
    .await;
    {
        let mut cfg = state.config.load().as_ref().clone();
        cfg.sandbox.waiting_input_timeout_seconds = 1;
        state.config.store(cfg.into());
    }
    let (nid, actors, cid) = base_novel(&state).await;
    let actor_ids: Vec<String> = actors.split(',').map(str::to_string).collect();
    let first = actor_ids[0].clone();
    // 替换为指向真实角色的导演响应
    let state = {
        let mut state = state;
        let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
        provider.push_ok(json!({ "next_actor_id": first, "reason": "等待", "request_user_input": { "question": "选择？" } }).to_string());
        provider.push_ok(outline_text());
        state.sessions = Arc::new(
            story_architect::services::sandbox::SessionManager::with_chain(
                state.pool.clone(),
                state.config.clone(),
                4,
                ProviderChain::new(vec![provider], 0, Duration::from_secs(2)),
            ),
        );
        state
    };
    let req = CreateSessionReq {
        actor_ids,
        mode: DirectorMode::Director,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(3),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::WaitingInput
    })
    .await;
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.status, SessionStatus::Completed);
}

#[tokio::test]
async fn scripted_plan_exhaustion_finalizes() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("脚本行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let actor_ids: Vec<String> = actors.split(',').map(str::to_string).collect();
    let first = actor_ids[0].clone();
    let req = CreateSessionReq {
        actor_ids,
        mode: DirectorMode::Scripted,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(3),
        director_config: Some(story_architect::models::sandbox::ScriptedPlan {
            steps: vec![story_architect::models::sandbox::ScriptStep {
                actor_id: first,
                beat_hint: Some("关键落点".to_string()),
            }],
        }),
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.current_step, 1);
    let cause: String = sqlx::query_scalar(
        "SELECT json_extract(payload, '$.cause') FROM sandbox_events WHERE session_id = ? AND event_type = 'finalize_started' LIMIT 1",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(cause, "scripted_done");
}
