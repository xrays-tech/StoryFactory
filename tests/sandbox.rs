// tests/sandbox.rs — P5 引擎集成测试（mock LLM，无网络）
mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use story_architect::llm::failover::ProviderChain;
use story_architect::llm::mock::MockProvider;
use story_architect::llm::{MessageRole, Purpose, StructuredMode};
use story_architect::models::sandbox::{
    ControlAction, CreateSessionReq, DirectorMode, SessionSnapshot, SessionStatus,
};
use story_architect::services::sandbox::engine::validate_outline;
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
    format!(
        "## 章节标题\n雨夜重逢\n\
         ## 剧情梗概\n{} 已推演的行动仅限厅内试探，未发生的门外对手戏与私盐定策不得写成既成事实。\n\
         ## 关键场景\n- 旧城车站：两人雨夜重逢，彼此试探来意与底牌。\n\
         ## 人物变化\n- 角色A：由戒备转为愿意倾听，但尚未交出全部秘密。\n\
         ## 差异清单\n### 未演\n- 门外对手戏：本场未推演。\n- 私盐定策：本场未推演。\n\
         ### 改写\n- 无\n\
         ## 衔接下一章\n两人约定明日再谈，门外来客尚未现身。",
        "这是测试大纲，用于验证收束写入流程与未覆盖情节的区分。".repeat(3)
    )
}

fn degraded_outline() -> String {
    format!(
        "⚠️ 降级模式（LLM 不可用）\n\n## 剧情梗概\n{}",
        "演员原始台词堆叠。".repeat(40)
    )
}

fn outline_without_diff_section() -> String {
    format!(
        "## 章节标题\n雨夜\n## 剧情梗概\n{}\n## 未覆盖要点\n无",
        "这段大纲长度足够但只有旧版未覆盖小节。".repeat(12)
    )
}

fn finalize_prompt(provider: &MockProvider) -> String {
    provider
        .captured_requests()
        .into_iter()
        .find(|(purpose, _)| *purpose == Purpose::Finalize)
        .map(|(_, messages)| {
            messages
                .into_iter()
                .map(|message| message.content)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .expect("应捕获到收束 prompt")
}

#[test]
fn validate_outline_covers_failure_conditions_and_pass_sample() {
    assert_eq!(
        validate_outline(&degraded_outline()),
        Err("输出包含降级或不可用标记")
    );
    let short = format!(
        "## 剧情梗概\n{}\n## 差异清单\n### 未演\n无\n### 改写\n无",
        "短".repeat(20)
    );
    assert_eq!(validate_outline(&short), Err("大纲正文不足 200 字"));
    assert_eq!(
        validate_outline(&outline_without_diff_section()),
        Err("缺少「差异清单」小节")
    );
    let missing_summary = format!(
        "## 章节标题\n雨夜\n## 差异清单\n### 未演\n无\n### 改写\n无\n{}",
        "内容。".repeat(80)
    );
    assert_eq!(
        validate_outline(&missing_summary),
        Err("缺少「剧情梗概」小节")
    );
    assert!(validate_outline(&outline_text()).is_ok());
}

fn director_prompt(provider: &MockProvider) -> String {
    provider
        .captured_requests()
        .into_iter()
        .find(|(purpose, _)| *purpose == Purpose::Director)
        .and_then(|(_, messages)| {
            messages
                .into_iter()
                .find(|message| message.role == MessageRole::System)
        })
        .map(|message| message.content)
        .expect("应捕获到导演 system prompt")
}

fn actor_prompt(provider: &MockProvider) -> String {
    provider
        .captured_requests()
        .into_iter()
        .find(|(purpose, _)| *purpose == Purpose::ActorStep)
        .map(|(_, messages)| {
            messages
                .into_iter()
                .map(|message| message.content)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .expect("应捕获到演员 prompt")
}

async fn set_chapter_beats(state: &AppState, chapter_id: &str, beats: &[&str], outline: &str) {
    sqlx::query("UPDATE chapters SET beats = ?, final_outline = ? WHERE id = ?")
        .bind(serde_json::to_string(beats).expect("beats json"))
        .bind(outline)
        .bind(chapter_id)
        .execute(&state.pool)
        .await
        .expect("update chapter beats");
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

async fn run_bound_finalize(state: &AppState, plan_outline: &str) -> (String, String) {
    let (nid, actors, cid) = base_novel(state).await;
    sqlx::query("UPDATE chapters SET final_outline = ? WHERE id = ?")
        .bind(plan_outline)
        .bind(&cid)
        .execute(&state.pool)
        .await
        .unwrap();
    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    wait_for(state, &created.id, |s| s.status == SessionStatus::Completed).await;
    (cid, created.id)
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
        chapter_id: None,
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
async fn finalize_indexes_generated_outline_for_search() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("A 的行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let app = test_app(state.clone());
    let (status, hits) = common::call(
        &app,
        axum::http::Method::GET,
        "/api/v1/search?q=测试大纲",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{hits}");
    assert_eq!(hits["total"], 1, "finalize 生成的大纲必须可被检索: {hits}");
    assert_eq!(hits["items"][0]["entity_type"], "chapter");
    assert_eq!(hits["items"][0]["entity_id"], cid);
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
        chapter_id: None,
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
async fn director_mode_accepts_actor_name_as_next_actor_id() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_ok(json!({ "next_actor_id": "角色B", "reason": "冲突" }).to_string());
    provider.push_ok(actor_json("B 的行动"));
    provider.push_ok(outline_text());
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let actor_ids: Vec<String> = actors.split(',').map(str::to_string).collect();
    let expected_actor = actor_ids[1].clone();
    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids,
        mode: DirectorMode::Director,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.current_step, 1);

    let action_payload: String = sqlx::query_scalar(
        "SELECT payload FROM sandbox_events WHERE session_id = ? AND event_type = 'actor_action' LIMIT 1",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let action: serde_json::Value = serde_json::from_str(&action_payload).unwrap();
    assert_eq!(
        action["actor_id"], expected_actor,
        "导演返回姓名时必须解析为对应角色 id"
    );

    let fallbacks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'director_fallback'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(fallbacks, 0, "姓名可解析时不应降级");

    let decision_payload: String = sqlx::query_scalar(
        "SELECT payload FROM sandbox_events WHERE session_id = ? AND event_type = 'director_decision' LIMIT 1",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let decision: serde_json::Value = serde_json::from_str(&decision_payload).unwrap();
    assert_eq!(decision["next_actor_id"], expected_actor);
}

#[tokio::test]
async fn director_mode_falls_back_on_unknown_actor_identifier() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_ok(json!({ "next_actor_id": "完全不存在", "reason": "冲突" }).to_string());
    provider.push_ok(actor_json("轮转行动"));
    provider.push_ok(outline_text());
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let actor_ids: Vec<String> = actors.split(',').map(str::to_string).collect();
    let expected_actor = actor_ids[0].clone();
    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids,
        mode: DirectorMode::Director,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let cause: String = sqlx::query_scalar(
        "SELECT json_extract(payload, '$.cause') FROM sandbox_events WHERE session_id = ? AND event_type = 'director_fallback' LIMIT 1",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(
        cause.contains("无法识别"),
        "降级原因应标明无法识别: {cause}"
    );
    assert!(
        cause.contains("完全不存在"),
        "降级原因应包含模型原始值: {cause}"
    );

    let action_payload: String = sqlx::query_scalar(
        "SELECT payload FROM sandbox_events WHERE session_id = ? AND event_type = 'actor_action' LIMIT 1",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let action: serde_json::Value = serde_json::from_str(&action_payload).unwrap();
    assert_eq!(action["actor_id"], expected_actor, "应降级为轮转选角");
}

#[tokio::test]
async fn step_executes_one_step_and_stays_paused() {
    let state = test_state_with_chain(Some(mock_chain(vec![actor_json("单步行动")]))).await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        chapter_id: None,
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
        chapter_id: None,
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
async fn in_flight_step_cannot_commit_after_abort() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let actor_ids: Vec<String> = actors.split(',').map(str::to_string).collect();
    let target = actor_ids[1].clone();
    provider.push_delay_ok(
        Duration::from_millis(500),
        json!({
            "actor_action": { "type": "dialogue", "content": "迟到的行动" },
            "emotion_delta": {
                "target_actor_id": target,
                "affinity_delta": 10,
                "trust_delta": 10
            }
        })
        .to_string(),
    );
    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids,
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(5),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 模拟竞态：Abort 状态已落库，但在途 LLM 结果尚未观察到取消令牌。
    sqlx::query("UPDATE sandbox_sessions SET status = 'aborted' WHERE id = ?")
        .bind(&created.id)
        .execute(&state.pool)
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(900)).await;
    let snap = state.sessions.snapshot(&created.id).await.unwrap();
    assert_eq!(snap.status, SessionStatus::Aborted);
    assert_eq!(snap.current_step, 0, "aborted session must not advance");

    let actions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'actor_action'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(actions, 0, "in-flight action must be dropped");
    let emotions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'emotion_updated'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(emotions, 0, "emotion update must be dropped");
    let relations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM character_relations WHERE novel_id = ?")
            .bind(&nid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(relations, 0, "relations must stay untouched");
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
        chapter_id: None,
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
        chapter_id: None,
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
        chapter_id: None,
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
        chapter_id: None,
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
        chapter_id: None,
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
async fn terminate_precedes_request_input() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    {
        let mut cfg = state.config.load().as_ref().clone();
        cfg.sandbox.waiting_input_timeout_seconds = 1;
        state.config.store(cfg.into());
    }
    let app = test_app(state.clone());
    let nid = create_novel(&app, "终止优先测试").await;
    let a = create_character(&app, &nid, "角色A").await;
    let vid = create_volume(&app, &nid, "卷一").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;

    provider.push_ok(
        json!({
            "next_actor_id": a,
            "reason": "收束",
            "terminate": true,
            "request_user_input": { "question": "继续吗？" }
        })
        .to_string(),
    );
    provider.push_ok(outline_text());
    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: vec![a],
        mode: DirectorMode::Director,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(3),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.status, SessionStatus::Completed);

    let waiting: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'waiting_input'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(waiting, 0, "terminate must win over request_user_input");

    let cause: String = sqlx::query_scalar(
        "SELECT json_extract(payload, '$.cause') FROM sandbox_events WHERE session_id = ? AND event_type = 'finalize_started' LIMIT 1",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(cause, "terminate");
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
        chapter_id: None,
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

#[tokio::test]
async fn negative_emotion_delta_on_new_pair_is_clamped() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        json!({
            "actor_action": { "type": "dialogue", "content": "试探" },
            "emotion_delta": { "target_actor_id": "TARGET", "affinity_delta": -5, "trust_delta": -10 }
        }).to_string(),
        outline_text(),
    ])))
    .await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "情感夹紧测试").await;
    let a = create_character(&app, &nid, "角色A").await;
    let b = create_character(&app, &nid, "角色B").await;
    let vid = create_volume(&app, &nid, "卷一").await;
    let cid = create_chapter(&app, &nid, &vid, "第一章").await;

    // 重写 mock 响应中的目标角色 id
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_ok(
        json!({
            "actor_action": { "type": "dialogue", "content": "试探" },
            "emotion_delta": { "target_actor_id": b, "affinity_delta": -5, "trust_delta": -10 }
        })
        .to_string(),
    );
    provider.push_ok(outline_text());
    let mut state = state;
    state.sessions = Arc::new(
        story_architect::services::sandbox::SessionManager::with_chain(
            state.pool.clone(),
            state.config.clone(),
            4,
            ProviderChain::new(vec![provider], 0, Duration::from_secs(2)),
        ),
    );
    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: vec![a.clone(), b.clone()],
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.status, SessionStatus::Completed);
    let (affinity, trust): (i64, i64) = sqlx::query_as(
        "SELECT affinity, trust FROM character_relations WHERE novel_id = ? AND char_a_id = ? AND char_b_id = ?",
    )
    .bind(&nid)
    .bind(if a < b { &a } else { &b })
    .bind(if a < b { &b } else { &a })
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(affinity, -5);
    assert_eq!(trust, 0, "negative trust must clamp to 0 on insert");
}

#[tokio::test]
async fn create_session_with_chapter_binds_and_finalizes() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("绑定章节的行动"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let req = CreateSessionReq {
        chapter_id: Some(cid.clone()),
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state
        .sessions
        .create(&nid, None, req)
        .await
        .expect("create");
    assert_eq!(
        created.chapter_id.as_deref(),
        Some(cid.as_str()),
        "创建返回体必须回显绑定的章节"
    );

    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let (outline, session_summary): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT c.final_outline, s.summary_text
         FROM chapters c JOIN sandbox_sessions s ON s.id = ?
         WHERE c.id = ?",
    )
    .bind(&created.id)
    .bind(&cid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let outline = outline.expect("绑定章节后收束必须写入 final_outline");
    assert!(!outline.trim().is_empty());
    assert_eq!(Some(&outline), session_summary.as_ref());

    let indexed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM search_docs WHERE entity_id = ?")
        .bind(&cid)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert!(indexed >= 1, "finalize 后必须重建章节 FTS 索引");

    let ready: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'outline_ready'",
    )
    .bind(&created.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(ready, 1, "绑定章节的会话必须持久化 outline_ready 事件");
}

#[tokio::test]
async fn finalize_rejects_degraded_output_and_keeps_chapter_outline() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("厅内试探"),
        degraded_outline(),
    ])))
    .await;
    let (cid, sid) = run_bound_finalize(&state, "原始章节大纲").await;

    let outline: Option<String> =
        sqlx::query_scalar("SELECT final_outline FROM chapters WHERE id = ?")
            .bind(&cid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        outline.as_deref(),
        Some("原始章节大纲"),
        "降级输出不得覆盖章节大纲"
    );

    let summary: Option<String> =
        sqlx::query_scalar("SELECT summary_text FROM sandbox_sessions WHERE id = ?")
            .bind(&sid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(
        summary.unwrap_or_default().contains("本次大纲未写入章节"),
        "会话摘要必须说明章节大纲未写入"
    );

    let skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'outline_skipped'",
    )
    .bind(&sid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(skipped, 1, "必须产生 outline_skipped 事件");
    let reason: String = sqlx::query_scalar(
        "SELECT json_extract(payload, '$.reason') FROM sandbox_events WHERE session_id = ? AND event_type = 'outline_skipped'",
    )
    .bind(&sid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(reason.contains("降级"), "跳过原因应标明降级标记: {reason}");
}

#[tokio::test]
async fn finalize_rejects_outline_without_diff_section() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("厅内试探"),
        outline_without_diff_section(),
    ])))
    .await;
    let (cid, sid) = run_bound_finalize(&state, "原始章节大纲").await;

    let outline: Option<String> =
        sqlx::query_scalar("SELECT final_outline FROM chapters WHERE id = ?")
            .bind(&cid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        outline.as_deref(),
        Some("原始章节大纲"),
        "缺少未覆盖小节的输出不得覆盖章节大纲"
    );

    let summary: Option<String> =
        sqlx::query_scalar("SELECT summary_text FROM sandbox_sessions WHERE id = ?")
            .bind(&sid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(summary.unwrap_or_default().contains("未写入章节"));

    let skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'outline_skipped'",
    )
    .bind(&sid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(skipped, 1);
}

#[tokio::test]
async fn finalize_writes_outline_with_diff_section() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("厅内试探"),
        outline_text(),
    ])))
    .await;
    let (cid, sid) = run_bound_finalize(&state, "原始章节大纲").await;

    let outline: Option<String> =
        sqlx::query_scalar("SELECT final_outline FROM chapters WHERE id = ?")
            .bind(&cid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(
        outline.unwrap_or_default().contains("## 差异清单"),
        "合格大纲必须写入章节"
    );

    let ready: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'outline_ready'",
    )
    .bind(&sid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(ready, 1);
    let skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sandbox_events WHERE session_id = ? AND event_type = 'outline_skipped'",
    )
    .bind(&sid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(skipped, 0);
}

#[tokio::test]
async fn finalize_prompt_marks_plan_vs_played() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_ok(actor_json("厅内试探，未出门"));
    provider.push_ok(outline_text());
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    run_bound_finalize(&state, "计划：门外对手戏与私盐定策").await;

    let prompt = finalize_prompt(&provider);
    for needle in [
        "既定大纲",
        "已推演",
        "差异清单",
        "未演",
        "改写",
        "门外对手戏",
        "私盐定策",
        "厅内试探",
    ] {
        assert!(
            prompt.contains(needle),
            "收束 prompt 缺少「{needle}」: {prompt}"
        );
    }
    assert!(
        !prompt.contains("{{"),
        "收束 prompt 不应残留模板变量: {prompt}"
    );
}

#[tokio::test]
async fn create_session_rejects_foreign_chapter() {
    let state = test_state_with_chain(None).await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "绑定目标小说").await;
    let actor = create_character(&app, &nid, "角色A").await;
    let other_nid = create_novel(&app, "其它小说").await;
    let other_vid = create_volume(&app, &other_nid, "其它卷").await;
    let foreign_cid = create_chapter(&app, &other_nid, &other_vid, "其它章").await;

    let req = CreateSessionReq {
        chapter_id: Some(foreign_cid),
        actor_ids: vec![actor],
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let err = state
        .sessions
        .create(&nid, None, req)
        .await
        .expect_err("跨小说章节必须被拒绝");
    assert!(
        matches!(err.code, story_architect::error::ErrorCode::ChapterNotFound),
        "unexpected error code: {:?}",
        err.code
    );
    assert_eq!(err.status, 404);

    let sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sandbox_sessions WHERE novel_id = ?")
            .bind(&nid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(sessions, 0, "校验失败时不得写入会话");
}

#[tokio::test]
async fn director_prompt_includes_global_plot_volume_and_previous_chapter() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "星海回声").await;
    let actor = create_character(&app, &nid, "角色A").await;
    let vol1 = create_volume(&app, &nid, "第一卷").await;
    create_chapter(&app, &nid, &vol1, "第一章 启程").await;
    create_chapter(&app, &nid, &vol1, "第二章 暗流").await;
    let vol2 = create_volume(&app, &nid, "第二卷 破晓").await;
    let prev_cid = create_chapter(&app, &nid, &vol2, "第三章 封锁").await;
    let current_cid = create_chapter(&app, &nid, &vol2, "第四章 归途").await;

    sqlx::query("UPDATE novels SET world_view = ?, background = ?, summary = ? WHERE id = ?")
        .bind("世界观：星海帝国与失落舰队对峙。")
        .bind("背景：边境星域战云密布。")
        .bind("简介：信使寻找失踪的指挥官。")
        .bind(&nid)
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE volumes SET summary = ? WHERE id = ?")
        .bind("卷摘要：主角重返边境。")
        .bind(&vol2)
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chapters SET final_outline = ? WHERE id = ?")
        .bind("上一章大纲：主角穿越封锁线。")
        .bind(&prev_cid)
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chapters SET final_outline = ? WHERE id = ?")
        .bind("本章既有大纲：抵达归途港。")
        .bind(&current_cid)
        .execute(&state.pool)
        .await
        .unwrap();

    provider.push_ok(json!({ "next_actor_id": actor, "reason": "推进本章目标" }).to_string());
    provider.push_ok(actor_json("抵达归途港"));
    provider.push_ok(outline_text());

    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: vec![actor],
        mode: DirectorMode::Director,
        scene_prompt: "归途港的雨夜，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state
        .sessions
        .create(&nid, Some(&current_cid), req)
        .await
        .unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let prompt = director_prompt(&provider);
    assert!(
        prompt.contains("星海帝国与失落舰队对峙"),
        "prompt 缺少作品世界观: {prompt}"
    );
    assert!(
        prompt.contains("第2卷《破晓》"),
        "prompt 缺少卷定位: {prompt}"
    );
    assert!(
        prompt.contains("本章：第2章《归途》"),
        "prompt 缺少当前章定位: {prompt}"
    );
    assert!(prompt.contains("← 当前章"), "prompt 未标出当前章: {prompt}");
    assert!(
        prompt.contains("上一章大纲：主角穿越封锁线。"),
        "prompt 缺少上一章大纲: {prompt}"
    );
    let global_at = prompt.find("# 全局剧情").expect("缺少全局剧情小节");
    let scene_at = prompt.find("# 场景").expect("缺少场景小节");
    assert!(global_at < scene_at, "全局剧情必须位于场景之前");
}

#[tokio::test]
async fn director_prompt_uses_recent_completed_session_as_previous_scene() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "无章节小说").await;
    let actor = create_character(&app, &nid, "角色A").await;
    sqlx::query("UPDATE novels SET world_view = ? WHERE id = ?")
        .bind("世界观：海上城邦。")
        .bind(&nid)
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sandbox_sessions(id, novel_id, scene_prompt, actor_ids, mode, status, step_limit, current_step, summary_text, finished_at)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%s','now'))",
    )
    .bind("prior-completed-session")
    .bind(&nid)
    .bind("码头分别")
    .bind("[]")
    .bind("director")
    .bind("completed")
    .bind(5_i64)
    .bind(1_i64)
    .bind("上一场：他们在码头分手")
    .execute(&state.pool)
    .await
    .unwrap();

    provider.push_ok(json!({ "next_actor_id": actor, "reason": "承接上一场" }).to_string());
    provider.push_ok(actor_json("码头重逢"));
    provider.push_ok(outline_text());

    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: vec![actor],
        mode: DirectorMode::Director,
        scene_prompt: "码头重逢，海风裹着咸味。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, None, req).await.unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let prompt = director_prompt(&provider);
    assert!(
        prompt.contains("上一场：他们在码头分手"),
        "prompt 的上一场景应回退到最近完成会话的摘要: {prompt}"
    );
    assert!(
        !prompt.contains("（无前置场景"),
        "存在历史会话时不应输出无前置场景文案"
    );
}

#[tokio::test]
async fn director_prompt_marks_missing_previous_scene() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "起点小说").await;
    let actor = create_character(&app, &nid, "角色A").await;

    provider.push_ok(json!({ "next_actor_id": actor, "reason": "开场" }).to_string());
    provider.push_ok(actor_json("序幕拉开"));
    provider.push_ok(outline_text());

    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: vec![actor],
        mode: DirectorMode::Director,
        scene_prompt: "故事开始，雨落在空荡的码头。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, None, req).await.unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let prompt = director_prompt(&provider);
    assert!(
        prompt.contains("（无前置场景，本场景为起点）"),
        "无上一章且无历史会话时应输出固定文案: {prompt}"
    );
}

#[tokio::test]
async fn create_session_with_blank_chapter_id_is_none() {
    let state = test_state_with_chain(None).await;
    let app = test_app(state.clone());
    let nid = create_novel(&app, "空白章节测试").await;
    let actor = create_character(&app, &nid, "角色A").await;

    let req = CreateSessionReq {
        chapter_id: Some("".to_string()),
        actor_ids: vec![actor],
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let created = state
        .sessions
        .create(&nid, None, req)
        .await
        .expect("create");
    assert_eq!(created.chapter_id, None);

    let stored: Option<String> =
        sqlx::query_scalar("SELECT chapter_id FROM sandbox_sessions WHERE id = ?")
            .bind(&created.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(stored, None, "空白 chapter_id 必须按未绑定落库");
}

#[tokio::test]
async fn director_prompt_lists_beats_with_indexes() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    let first = actors.split(',').next().unwrap().to_string();
    set_chapter_beats(
        &state,
        &cid,
        &["雨夜重逢", "门外来客"],
        "## 剧情梗概\n两人在旧城车站重逢。",
    )
    .await;

    provider.push_ok(
        json!({ "next_actor_id": first, "next_beat": 0, "reason": "推进本章情节" }).to_string(),
    );
    provider.push_ok(actor_json("旧城车站重逢"));
    provider.push_ok(outline_text());

    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Director,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let prompt = director_prompt(&provider);
    assert!(
        prompt.contains("[0]"),
        "导演 prompt 缺少情节下标 0: {prompt}"
    );
    assert!(
        prompt.contains("[1]"),
        "导演 prompt 缺少情节下标 1: {prompt}"
    );
    assert!(
        prompt.contains("雨夜重逢"),
        "导演 prompt 缺少情节文本: {prompt}"
    );
    assert!(
        prompt.contains("门外来客"),
        "导演 prompt 缺少情节文本: {prompt}"
    );
}

#[tokio::test]
async fn actor_receives_current_beat_and_outline() {
    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    let state = test_state_with_chain(Some(ProviderChain::new(
        vec![provider.clone()],
        0,
        Duration::from_secs(2),
    )))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    set_chapter_beats(
        &state,
        &cid,
        &["雨夜重逢"],
        "## 剧情梗概\n两人在旧城车站重逢，彼此试探来意。",
    )
    .await;

    provider.push_ok(actor_json("旧城车站重逢"));
    provider.push_ok(outline_text());

    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(1),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;

    let prompt = actor_prompt(&provider);
    assert!(
        prompt.contains("# 本步情节"),
        "演员 prompt 缺少本步情节段: {prompt}"
    );
    assert!(
        prompt.contains("雨夜重逢"),
        "演员 prompt 缺少当前 beat 文本: {prompt}"
    );
    assert!(
        prompt.contains("# 本章大纲"),
        "演员 prompt 缺少本章大纲段: {prompt}"
    );
    assert!(
        prompt.contains("彼此试探来意"),
        "演员 prompt 缺少本章大纲文本: {prompt}"
    );
}

#[tokio::test]
async fn step_limit_defaults_from_beat_count() {
    let state = test_state_with_chain(None).await;
    let (nid, actors, cid) = base_novel(&state).await;
    let beats = [
        "情节一",
        "情节二",
        "情节三",
        "情节四",
        "情节五",
        "情节六",
        "情节七",
        "情节八",
    ];
    set_chapter_beats(&state, &cid, &beats, "## 剧情梗概\n八个情节待演绎。").await;

    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: None,
        director_config: None,
        user_directive: None,
        autostart: Some(false),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();

    assert_eq!(created.step_limit, 10, "8 个 beat 应得到 ceil(9.6)=10 步");
    assert_eq!(created.beats_total, 8);
    assert_eq!(created.beats_done_count, 0);
}

#[tokio::test]
async fn beats_done_accumulates() {
    let state = test_state_with_chain(Some(mock_chain(vec![
        actor_json("第一步"),
        actor_json("第二步"),
        actor_json("第三步"),
        outline_text(),
    ])))
    .await;
    let (nid, actors, cid) = base_novel(&state).await;
    set_chapter_beats(
        &state,
        &cid,
        &["情节一", "情节二", "情节三", "情节四", "情节五", "情节六"],
        "## 剧情梗概\n六个情节待演绎。",
    )
    .await;

    let req = CreateSessionReq {
        chapter_id: None,
        actor_ids: actors.split(',').map(str::to_string).collect(),
        mode: DirectorMode::Roundrobin,
        scene_prompt: "雨夜的旧城车站，两人重逢。".to_string(),
        step_limit: Some(3),
        director_config: None,
        user_directive: None,
        autostart: Some(true),
    };
    let created = state.sessions.create(&nid, Some(&cid), req).await.unwrap();
    let done = wait_for(&state, &created.id, |s| {
        s.status == SessionStatus::Completed
    })
    .await;
    assert_eq!(done.current_step, 3);

    let raw: Option<String> =
        sqlx::query_scalar("SELECT beats_done FROM sandbox_sessions WHERE id = ?")
            .bind(&created.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    let indices: Vec<usize> = serde_json::from_str(raw.as_deref().unwrap_or("[]")).unwrap();
    assert_eq!(indices.len(), 3, "三步应消费三个情节: {indices:?}");
    let mut unique = indices.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 3, "情节下标不得重复: {indices:?}");
    assert!(unique.iter().all(|index| *index < 6));
}

#[test]
fn validate_outline_accepts_diff_section() {
    let text = format!(
        "## 剧情梗概\n{}\n## 差异清单\n### 未演\n无\n### 改写\n无",
        "这一幕已经发生。".repeat(40)
    );
    assert!(validate_outline(&text).is_ok());
}

#[test]
fn validate_outline_rejects_legacy_uncovered_heading() {
    let text = format!(
        "## 剧情梗概\n{}\n## 未覆盖要点\n- 旧版小节不应再被接受。",
        "这一幕已经发生。".repeat(40)
    );
    assert_eq!(validate_outline(&text), Err("缺少「差异清单」小节"));
}
