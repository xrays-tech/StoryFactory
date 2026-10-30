// src/services/sandbox/actor.rs — 单角色执行（§7.5 / §8.2）
use serde_json::json;

use crate::config::AppConfig;
use crate::error::{AppError, ErrorCode};
use crate::llm::budget::Budget;
use crate::llm::schemas::actor_step_schema;
use crate::llm::service::LlmService;
use crate::llm::{Message, Purpose};
use crate::services::sandbox::context::{self, AgentCard, HistoryStep};

#[derive(Debug, Clone)]
pub struct ActorOutcome {
    pub actor_id: String,
    pub actor_name: String,
    pub action_type: String,
    pub content: String,
    pub target_actor_id: Option<String>,
    pub affinity_delta: i64,
    pub trust_delta: i64,
    pub director_note: Option<String>,
    pub step: u32,
}

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    cards: &[AgentCard],
    actor_id: &str,
    step: u32,
    scene: &str,
    user_directive: &str,
    rolling_summary: &str,
    history: &[HistoryStep],
    emotion_matrix: &str,
    directive: &str,
    llm: &LlmService,
    budget: &Budget,
    cfg: &AppConfig,
) -> Result<ActorOutcome, AppError> {
    let card = cards
        .iter()
        .find(|c| c.id == actor_id)
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "角色卡缺失"))?;

    let system_ctx = json!({
        "self_card": context::self_card_text(card),
        "scene": scene,
        "user_directive": user_directive,
        "emotion_matrix": emotion_matrix,
    });
    let user_ctx = json!({
        "rolling_summary": rolling_summary,
        "recent_history": history,
        "directive": directive,
    });
    let system_prompt = crate::llm::prompts::render(crate::llm::prompts::ACTOR_STEP, &system_ctx)?;
    let user_prompt = crate::llm::prompts::render(crate::llm::prompts::ACTOR_STEP_USER, &user_ctx)?;

    let (value, _attempts) = llm
        .complete_structured(
            budget,
            Purpose::ActorStep,
            vec![Message::system(system_prompt), Message::user(user_prompt)],
            actor_step_schema(),
            2,
        )
        .await
        .map_err(|e| AppError::new(ErrorCode::LlmUnavailable, format!("演员调用失败: {e}")))?;

    let action_type = value["actor_action"]["type"]
        .as_str()
        .unwrap_or("action")
        .to_string();
    let content = value["actor_action"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if content.trim().is_empty() {
        return Err(AppError::new(ErrorCode::LlmUnavailable, "演员输出内容为空"));
    }
    let target = value["emotion_delta"]["target_actor_id"]
        .as_str()
        .map(str::to_string);
    let affinity = value["emotion_delta"]["affinity_delta"]
        .as_i64()
        .unwrap_or(0)
        .clamp(-20, 20);
    let trust = value["emotion_delta"]["trust_delta"]
        .as_i64()
        .unwrap_or(0)
        .clamp(-20, 20);
    let note = value["director_note"].as_str().map(str::to_string);

    let _ = cfg;
    Ok(ActorOutcome {
        actor_id: card.id.clone(),
        actor_name: card.name.clone(),
        action_type,
        content,
        target_actor_id: target,
        affinity_delta: affinity,
        trust_delta: trust,
        director_note: note,
        step,
    })
}
