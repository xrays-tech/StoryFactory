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
    pub global_plot: &'a str,
    pub previous_scene: &'a str,
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

fn resolve_actor_id(raw: &str, actor_ids: &[String], cards: &[AgentCard]) -> Option<String> {
    let candidate = raw.trim();
    if candidate.is_empty() {
        return None;
    }
    if let Some(id) = actor_ids
        .iter()
        .find(|id| id.eq_ignore_ascii_case(candidate))
    {
        return Some(id.clone());
    }
    if let Some(card) = cards
        .iter()
        .find(|card| card.id.eq_ignore_ascii_case(candidate))
    {
        return Some(card.id.clone());
    }
    cards
        .iter()
        .find(|card| card.name.trim().to_lowercase() == candidate.to_lowercase())
        .map(|card| card.id.clone())
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
                "global_plot": input.global_plot,
                "previous_scene": input.previous_scene,
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
            let raw_actor_id = value["next_actor_id"].as_str().unwrap_or("");
            let (mut actor_id, mut fallback, mut reason) =
                match resolve_actor_id(raw_actor_id, input.actor_ids, input.cards) {
                    Some(actor_id) => (actor_id, false, None),
                    None => {
                        let shown: String = raw_actor_id.trim().chars().take(40).collect();
                        let cause = format!("导演返回了无法识别的角色标识：{shown}");
                        let actor_id = roundrobin(input, true, Some(cause.clone())).actor_id;
                        (actor_id, true, Some(cause))
                    }
                };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, name: &str) -> AgentCard {
        AgentCard {
            id: id.to_string(),
            name: name.to_string(),
            role_type: "support".to_string(),
            appearance: None,
            personality: None,
            hidden_personality: None,
            biography: None,
            current_goal: None,
            speaking_style: None,
        }
    }

    #[test]
    fn resolve_actor_id_matches_exact_and_case_insensitive_id() {
        let actor_ids = vec!["A1B2C3".to_string(), "d4e5f6".to_string()];
        let cards = vec![card("A1B2C3", "甲"), card("d4e5f6", "乙")];

        assert_eq!(
            resolve_actor_id("A1B2C3", &actor_ids, &cards).as_deref(),
            Some("A1B2C3")
        );
        assert_eq!(
            resolve_actor_id("a1b2c3", &actor_ids, &cards).as_deref(),
            Some("A1B2C3")
        );
    }

    #[test]
    fn resolve_actor_id_matches_card_id_when_actor_list_misses() {
        let actor_ids: Vec<String> = Vec::new();
        let cards = vec![card("CARD-1", "甲")];

        assert_eq!(
            resolve_actor_id("CARD-1", &actor_ids, &cards).as_deref(),
            Some("CARD-1")
        );
    }

    #[test]
    fn resolve_actor_id_matches_name() {
        let actor_ids = vec!["id-a".to_string()];
        let cards = vec![card("id-a", "角色A")];

        assert_eq!(
            resolve_actor_id("角色A", &actor_ids, &cards).as_deref(),
            Some("id-a")
        );
    }

    #[test]
    fn resolve_actor_id_matches_name_with_surrounding_spaces() {
        let actor_ids = vec!["id-a".to_string()];
        let cards = vec![card("id-a", " 角色A ")];

        assert_eq!(
            resolve_actor_id("  角色A  ", &actor_ids, &cards).as_deref(),
            Some("id-a")
        );
    }

    #[test]
    fn resolve_actor_id_returns_none_for_unknown_value() {
        let actor_ids = vec!["id-a".to_string()];
        let cards = vec![card("id-a", "角色A")];

        assert_eq!(resolve_actor_id("并不存在", &actor_ids, &cards), None);
        assert_eq!(resolve_actor_id("", &actor_ids, &cards), None);
    }

    #[test]
    fn resolve_actor_id_uses_first_card_on_duplicate_names() {
        let actor_ids = vec!["id-1".to_string(), "id-2".to_string()];
        let cards = vec![card("id-1", "同名"), card("id-2", "同名")];

        assert_eq!(
            resolve_actor_id("同名", &actor_ids, &cards).as_deref(),
            Some("id-1")
        );
    }
}
