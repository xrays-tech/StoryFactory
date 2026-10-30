// src/services/sandbox/director.rs — 三模式选角 + 防重复（§8.2）
use serde_json::{json, Value};

use crate::config::AppConfig;
use crate::llm::budget::Budget;
use crate::llm::schemas::director_schema;
use crate::llm::service::LlmService;
use crate::llm::{Message, Purpose};
use crate::models::sandbox::{DirectorMode, ScriptedPlan};
use crate::services::sandbox::context::{actor_cards_brief, AgentCard, HistoryStep};

#[derive(Debug, Clone)]
pub struct DirectorDecision {
    pub actor_id: String,
    pub hint: Option<String>,
    pub terminate: bool,
    pub request_input: Option<(String, Vec<String>)>,
    pub fallback: bool,
    pub fallback_reason: Option<String>,
    pub scripted_done: bool,
}

pub struct DirectorInput<'a> {
    pub mode: DirectorMode,
    pub actor_ids: &'a [String],
    pub current_step: u32,
    pub step_limit: u32,
    pub scripted: Option<&'a ScriptedPlan>,
    pub cards: &'a [AgentCard],
    pub history: &'a [HistoryStep],
    pub rolling_summary: &'a str,
    pub emotion_matrix: &'a str,
    pub scene: &'a str,
    pub last_actor: Option<&'a str>,
    pub consecutive: u32,
}

fn roundrobin(
    input: &DirectorInput<'_>,
    fallback: bool,
    reason: Option<String>,
) -> DirectorDecision {
    let actor_id = input
        .actor_ids
        .get(input.current_step as usize % input.actor_ids.len().max(1))
        .cloned()
        .unwrap_or_default();
    DirectorDecision {
        actor_id,
        hint: None,
        terminate: false,
        request_input: None,
        fallback,
        fallback_reason: reason,
        scripted_done: false,
    }
}

pub async fn decide(
    input: &DirectorInput<'_>,
    cfg: &AppConfig,
    llm: &LlmService,
    budget: &Budget,
) -> DirectorDecision {
    match input.mode {
        DirectorMode::Roundrobin => roundrobin(input, false, None),
        DirectorMode::Scripted => {
            if let Some(plan) = input.scripted {
                if let Some(step) = plan.steps.get(input.current_step as usize) {
                    return DirectorDecision {
                        actor_id: step.actor_id.clone(),
                        hint: step.beat_hint.clone(),
                        terminate: false,
                        request_input: None,
                        fallback: false,
                        fallback_reason: None,
                        scripted_done: false,
                    };
                }
            }
            DirectorDecision {
                actor_id: String::new(),
                hint: None,
                terminate: true,
                request_input: None,
                fallback: false,
                fallback_reason: Some("scripted_done".to_string()),
                scripted_done: true,
            }
        }
        DirectorMode::Director => {
            let ctx = json!({
                "scene": input.scene,
                "actor_cards_brief": actor_cards_brief(input.cards),
                "emotion_matrix": input.emotion_matrix,
                "current_step": input.current_step,
                "step_limit": input.step_limit,
                "repeat_limit": cfg.sandbox.director_repeat_limit,
                "consecutive_steps": input.consecutive,
                "rolling_summary": input.rolling_summary,
                "recent_history": input.history,
            });
            let prompt =
                match crate::llm::prompts::render(crate::llm::prompts::DIRECTOR_DECIDE, &ctx) {
                    Ok(p) => p,
                    Err(e) => {
                        return roundrobin(input, true, Some(format!("导演模板渲染失败: {e}")))
                    }
                };
            let value = match llm
                .complete_structured(
                    budget,
                    Purpose::Director,
                    vec![Message::system(prompt)],
                    director_schema(),
                    2,
                )
                .await
            {
                Ok((value, _)) => value,
                Err(e) => return roundrobin(input, true, Some(format!("导演调用失败: {e}"))),
            };

            let terminate = value["terminate"].as_bool().unwrap_or(false);
            // terminate 优先：两者同时出现时直接收尾，绝不进入等待输入。
            let request_input = if terminate {
                None
            } else {
                value
                    .get("request_user_input")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        let question = obj
                            .get("question")
                            .and_then(|v| v.as_str())
                            .unwrap_or("请选择剧情走向")
                            .to_string();
                        let suggestions = obj
                            .get("suggestions")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(str::to_string))
                                    .take(4)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        (question, suggestions)
                    })
            };
            let hint = value["inject_scene_hint"]
                .as_str()
                .map(str::to_string)
                .or_else(|| value["reason"].as_str().map(str::to_string));
            let mut actor_id = value["next_actor_id"].as_str().unwrap_or("").to_string();

            let mut fallback = false;
            let mut reason = None;
            if !input.actor_ids.iter().any(|id| id == &actor_id) {
                actor_id =
                    roundrobin(input, true, Some("导演选择了不在场角色".to_string())).actor_id;
                fallback = true;
                reason = Some("导演选择了不在场角色".to_string());
            }
            if input.last_actor == Some(actor_id.as_str())
                && input.consecutive >= cfg.sandbox.director_repeat_limit
            {
                let replacement = input
                    .actor_ids
                    .iter()
                    .find(|id| id.as_str() != actor_id.as_str())
                    .cloned()
                    .unwrap_or(actor_id);
                actor_id = replacement;
                fallback = true;
                reason = Some("同一角色连续行动达到上限".to_string());
            }

            DirectorDecision {
                actor_id,
                hint,
                terminate,
                request_input,
                fallback,
                fallback_reason: reason,
                scripted_done: false,
            }
        }
    }
}

pub fn decision_event_payload(decision: &DirectorDecision, step: u32) -> Value {
    json!({
        "step": step,
        "next_actor_id": decision.actor_id,
        "reason": decision.hint.clone().unwrap_or_default(),
        "hint": decision.hint,
    })
}
