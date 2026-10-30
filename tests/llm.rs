// tests/llm.rs — LLM 层：结构化修复、故障转移、预算、模板、usage
use std::sync::Arc;
use std::time::{Duration, Instant};

use story_architect::llm::budget::Budget;
use story_architect::llm::factory;
use story_architect::llm::failover::ProviderChain;
use story_architect::llm::mock::MockProvider;
use story_architect::llm::prompts;
use story_architect::llm::schemas::{actor_step_schema, director_schema};
use story_architect::llm::service::LlmService;
use story_architect::llm::structured::{complete_json, extract_json};
use story_architect::llm::usage;
use story_architect::llm::{
    CompletionReq, LlmError, LlmErrorKind, LlmProvider, Message, Purpose, StructuredMode,
};

fn deadline(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

fn valid_actor_json() -> &'static str {
    r#"{"actor_action":{"type":"dialogue","content":"你好"},"emotion_delta":{"target_actor_id":"a","affinity_delta":1,"trust_delta":2}}"#
}

#[tokio::test]
async fn structured_repairs_invalid_output() {
    let provider = MockProvider::new("mock", StructuredMode::JsonObject);
    provider.push_ok("这不是 JSON");
    provider.push_ok(valid_actor_json());
    let req = CompletionReq::new(vec![Message::system("s")], deadline(5), Purpose::ActorStep);
    let value = complete_json(&provider, req, actor_step_schema(), 2)
        .await
        .expect("repair should succeed");
    assert_eq!(value["actor_action"]["content"], "你好");
    assert_eq!(provider.call_count(), 2);
}

#[tokio::test]
async fn structured_repair_exhausted() {
    let provider = MockProvider::new("mock", StructuredMode::JsonObject);
    provider.push_ok("{}");
    provider.push_ok("{}");
    let req = CompletionReq::new(vec![Message::system("s")], deadline(5), Purpose::ActorStep);
    let err = complete_json(&provider, req, actor_step_schema(), 1)
        .await
        .expect_err("must fail");
    assert_eq!(err.kind, LlmErrorKind::InvalidOutput);
    assert_eq!(provider.call_count(), 2);
}

#[tokio::test]
async fn structured_accepts_fenced_json() {
    let provider = MockProvider::new("mock", StructuredMode::JsonSchema);
    provider.push_ok(format!("```json\n{}\n```", valid_actor_json()));
    let req = CompletionReq::new(vec![Message::system("s")], deadline(5), Purpose::ActorStep);
    let value = complete_json(&provider, req, actor_step_schema(), 0)
        .await
        .expect("fenced JSON");
    assert_eq!(value["emotion_delta"]["affinity_delta"], 1);
}

#[test]
fn extract_json_handles_fences() {
    let value = extract_json("```json\n{\"a\":1}\n```").unwrap();
    assert_eq!(value["a"], 1);
    assert!(extract_json("not json").is_none());
}

#[tokio::test]
async fn failover_uses_fallback() {
    let primary = Arc::new(MockProvider::new("p1", StructuredMode::JsonObject));
    primary.push_err(LlmError::new(LlmErrorKind::RateLimited, "429"));
    let fallback = Arc::new(MockProvider::new("p2", StructuredMode::JsonObject));
    fallback.push_ok(valid_actor_json());
    let chain = ProviderChain::new(
        vec![primary.clone(), fallback.clone()],
        0,
        Duration::from_secs(2),
    );
    let req = CompletionReq::new(vec![Message::system("s")], deadline(5), Purpose::ActorStep);
    let outcome = chain.complete(req).await.expect("fallback");
    assert_eq!(outcome.provider, "p2");
    assert_eq!(outcome.attempts.len(), 2);
    assert_eq!(fallback.call_count(), 1);
}

#[tokio::test]
async fn failover_stops_on_auth_error() {
    let primary = Arc::new(MockProvider::new("p1", StructuredMode::JsonObject));
    primary.push_err(LlmError::new(LlmErrorKind::Auth, "401"));
    let fallback = Arc::new(MockProvider::new("p2", StructuredMode::JsonObject));
    fallback.push_ok(valid_actor_json());
    let chain = ProviderChain::new(vec![primary, fallback.clone()], 0, Duration::from_secs(2));
    let req = CompletionReq::new(vec![Message::system("s")], deadline(5), Purpose::ActorStep);
    let err = chain.complete(req).await.expect_err("auth must stop");
    assert_eq!(err.kind, LlmErrorKind::Auth);
    assert_eq!(fallback.call_count(), 0);
}

#[test]
fn budget_relationships() {
    let budget = Budget::new(1800, 90, 45);
    assert!(budget.attempt_deadline() <= budget.step_deadline);
    assert!(!budget.exhausted());
    assert!(budget.can_retry(Duration::from_secs(1)));
    assert!(budget.remaining() > Duration::from_secs(80));
}

#[test]
fn prompt_renders_strict_variables() {
    let out = prompts::render(
        "角色 {{ name }} / 第 {{ step }} 步",
        &serde_json::json!({ "name": "A", "step": 3 }),
    )
    .expect("render");
    assert_eq!(out, "角色 A / 第 3 步");
    let err = prompts::render("{{ missing }}", &serde_json::json!({}));
    assert!(err.is_err(), "undefined variable must error");
}

#[test]
fn schemas_are_valid_json() {
    let actor = actor_step_schema();
    let director = director_schema();
    assert_eq!(actor["title"], "ActorStepOutput");
    assert_eq!(director["title"], "DirectorDecision");
    assert!(director["properties"]["request_user_input"].is_object());
}

#[tokio::test]
async fn usage_records_and_prices() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().display().to_string();
    let pool = story_architect::db::open_pool(&data_dir).await.unwrap();
    story_architect::db::migrate(&pool).await.unwrap();
    sqlx::query("INSERT INTO app_config(key, value, is_secret) VALUES('llm_pricing', ?, 0)")
        .bind(r#"{"deepseek/deepseek-chat":{"input":0.27,"output":1.10,"currency_per":1000000}}"#)
        .execute(&pool)
        .await
        .unwrap();

    let cost = usage::cost_usd(&pool, "deepseek", "deepseek-chat", 1000, 500).await;
    assert!((cost - 0.00082).abs() < 1e-9, "cost={cost}");

    usage::record(
        &pool,
        None,
        "deepseek",
        "deepseek-chat",
        Purpose::ActorStep,
        1000,
        500,
        cost,
        123,
        true,
        None,
    )
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM llm_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn prompt_seed_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().display().to_string();
    let pool = story_architect::db::open_pool(&data_dir).await.unwrap();
    story_architect::db::migrate(&pool).await.unwrap();
    prompts::seed_defaults(&pool).await.unwrap();
    prompts::seed_defaults(&pool).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM prompt_templates WHERE is_active = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 5);
    let versions = prompts::snapshot_versions(&pool).await.unwrap();
    assert_eq!(versions["actor_step"], 1);
}

#[test]
fn provider_trait_object_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockProvider>();
    let _: Option<Arc<dyn LlmProvider>> = None;
}

#[test]
fn factory_without_key_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("APP_DATA_DIR", dir.path());
    let cfg = story_architect::config::load().unwrap();
    std::env::remove_var("APP_DATA_DIR");
    let chain = factory::build_chain(&cfg);
    assert!(chain.is_empty());
}

#[tokio::test]
async fn llm_service_records_usage_and_returns_value() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().display().to_string();
    let pool = story_architect::db::open_pool(&data_dir).await.unwrap();
    story_architect::db::migrate(&pool).await.unwrap();

    let provider = Arc::new(MockProvider::new("mock", StructuredMode::JsonObject));
    provider.push_ok(valid_actor_json());
    let chain = ProviderChain::new(vec![provider], 0, Duration::from_secs(2));
    assert_eq!(chain.attempt_timeout(), Duration::from_secs(2));
    let service = LlmService::new(chain, pool.clone(), None);
    let budget = Budget::new(60, 30, 10);
    let (value, attempts) = service
        .complete_structured(
            &budget,
            Purpose::ActorStep,
            vec![Message::system("s")],
            actor_step_schema(),
            0,
        )
        .await
        .expect("structured");
    assert_eq!(value["actor_action"]["content"], "你好");
    assert_eq!(attempts.len(), 1);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM llm_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
