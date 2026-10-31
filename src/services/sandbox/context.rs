// src/services/sandbox/context.rs — 角色卡/情感矩阵/历史与滚动摘要（§7.5）
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;

use crate::config::AppConfig;
use crate::db;
use crate::error::AppError;
use crate::llm::budget::Budget;
use crate::llm::service::LlmService;
use crate::llm::{Message, Purpose};
use crate::models::sandbox::{SandboxEventType, SessionRow};
use crate::services::sandbox::events::persist_event;
use crate::services::util::db_err;

type AgentCardRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(Debug, Clone, Serialize)]
pub struct AgentCard {
    pub id: String,
    pub name: String,
    pub role_type: String,
    pub appearance: Option<String>,
    pub personality: Option<String>,
    pub hidden_personality: Option<String>,
    pub biography: Option<String>,
    pub current_goal: Option<String>,
    pub speaking_style: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryStep {
    pub seq: i64,
    pub no: u32,
    pub actor_id: String,
    pub actor_name: String,
    pub action_type: String,
    pub content: String,
    pub emotion_note: String,
}

pub async fn load_agent_cards(
    pool: &SqlitePool,
    actor_ids: &[String],
) -> Result<Vec<AgentCard>, AppError> {
    let mut cards = Vec::new();
    for id in actor_ids {
        let row: Option<AgentCardRow> = sqlx::query_as(
            "SELECT id, name, role_type, appearance, personality, hidden_personality, biography, current_goal, speaking_style FROM characters WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        if let Some((
            id,
            name,
            role_type,
            appearance,
            personality,
            hidden_personality,
            biography,
            current_goal,
            speaking_style,
        )) = row
        {
            cards.push(AgentCard {
                id,
                name,
                role_type,
                appearance,
                personality,
                hidden_personality,
                biography,
                current_goal,
                speaking_style,
            });
        }
    }
    Ok(cards)
}

pub fn self_card_text(card: &AgentCard) -> String {
    let text = serde_json::to_string_pretty(card).unwrap_or_else(|_| card.name.clone());
    // §7.5：角色卡预算约 800 token；超出按 1600 字符近似截断。
    if estimate_tokens(&text) > 800 {
        text.chars().take(1600).collect()
    } else {
        text
    }
}

pub fn actor_cards_brief(cards: &[AgentCard]) -> String {
    cards
        .iter()
        .map(|c| {
            format!(
                "- id={}｜{}（{}）：{}",
                c.id,
                c.name,
                c.role_type,
                c.current_goal.as_deref().unwrap_or("暂无明确目标")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn emotion_matrix(
    pool: &SqlitePool,
    novel_id: &str,
    actor_ids: &[String],
) -> Result<String, AppError> {
    let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT char_a_id, char_b_id, affinity, trust FROM character_relations WHERE novel_id = ? ORDER BY char_a_id, char_b_id",
    )
    .bind(novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let allowed: std::collections::HashSet<&str> = actor_ids.iter().map(String::as_str).collect();
    let filtered: Vec<Value> = rows
        .into_iter()
        .filter(|(a, b, _, _)| allowed.contains(a.as_str()) && allowed.contains(b.as_str()))
        .map(|(a, b, affinity, trust)| {
            json!({ "a": a, "b": b, "affinity": affinity, "trust": trust })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&filtered).unwrap_or_else(|_| "[]".to_string()))
}

/// 按 token 预算从最新事件向前截断，返回时间正序窗口（§7.5 ⑥ ≤5500）。
pub async fn load_history_window(
    pool: &SqlitePool,
    session_id: &str,
    after_seq: i64,
    max_tokens: usize,
) -> Result<Vec<HistoryStep>, AppError> {
    let mut steps = load_history(pool, session_id, after_seq, 2000).await?;
    let mut total = 0;
    let mut start = steps.len();
    for (idx, step) in steps.iter().enumerate().rev() {
        let cost = estimate_tokens(&step.content) + 8;
        if total + cost > max_tokens {
            break;
        }
        total += cost;
        start = idx;
    }
    Ok(steps.split_off(start.min(steps.len())))
}

pub async fn load_history(
    pool: &SqlitePool,
    session_id: &str,
    after_seq: i64,
    limit: i64,
) -> Result<Vec<HistoryStep>, AppError> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT seq, payload FROM sandbox_events WHERE session_id = ? AND event_type = 'actor_action' AND seq > ? ORDER BY seq DESC LIMIT ?",
    )
    .bind(session_id)
    .bind(after_seq)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut steps = Vec::new();
    for (seq, payload) in rows.into_iter().rev() {
        let value: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
        let actor_id = value["actor_id"].as_str().unwrap_or("").to_string();
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM characters WHERE id = ?")
            .bind(&actor_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
        steps.push(HistoryStep {
            seq,
            no: value["step"].as_u64().unwrap_or(seq as u64) as u32,
            actor_id,
            actor_name: name.unwrap_or_else(|| "(已删除角色)".to_string()),
            action_type: value["action"]["type"]
                .as_str()
                .unwrap_or("action")
                .to_string(),
            content: value["action"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            emotion_note: value["emotion_delta"]
                .as_object()
                .map(|_| {
                    format!(
                        "affinity {:+}, trust {:+}",
                        value["emotion_delta"]["affinity_delta"]
                            .as_i64()
                            .unwrap_or(0),
                        value["emotion_delta"]["trust_delta"].as_i64().unwrap_or(0)
                    )
                })
                .unwrap_or_default(),
        });
    }
    Ok(steps)
}

pub fn estimate_tokens(text: &str) -> usize {
    // 中文场景近似：字符数 / 2（P5 采用保守估算，后续可替换 tiktoken）。
    text.chars().count() / 2 + 1
}

pub async fn maybe_compress(
    pool: &SqlitePool,
    session_id: &str,
    row: &SessionRow,
    llm: &LlmService,
    budget: &Budget,
    cfg: &AppConfig,
) -> Result<Option<i64>, AppError> {
    let summary = row.rolling_summary.clone().unwrap_or_default();
    let history = load_history(pool, session_id, row.summary_until_seq, 200).await?;
    let history_text = history
        .iter()
        .map(|s| format!("第{}步 · {}：{}", s.no, s.actor_name, s.content))
        .collect::<Vec<_>>()
        .join("\n");
    let estimated = estimate_tokens(&summary) + estimate_tokens(&history_text);
    if estimated <= cfg.sandbox.history_max_tokens as usize {
        return Ok(None);
    }

    let mut picked: Vec<&HistoryStep> = Vec::new();
    let mut remaining = estimated;
    for step in &history {
        if remaining <= 4000 {
            break;
        }
        let cost = estimate_tokens(&step.content) + 8;
        picked.push(step);
        remaining = remaining.saturating_sub(cost);
    }
    if picked.is_empty() {
        return Ok(None);
    }
    let until_seq = picked
        .last()
        .map(|step| step.seq)
        .unwrap_or(row.summary_until_seq);

    let ctx = json!({
        "previous_summary": summary,
        "steps": picked,
    });
    let body = crate::llm::prompts::ROLLING_SUMMARY;
    let prompt = crate::llm::prompts::render(body, &ctx)?;
    let summary_text = match llm
        .complete_text(
            budget,
            Purpose::Summary,
            vec![Message::user(prompt)],
            cfg.sandbox.rolling_summary_max_tokens,
        )
        .await
    {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(error = %e, "滚动摘要 LLM 失败，使用确定性截断");
            picked
                .iter()
                .map(|s| format!("第{}步 {}：{}", s.no, s.actor_name, s.content))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    sqlx::query(
        "UPDATE sandbox_sessions SET rolling_summary = ?, summary_until_seq = ?, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(&summary_text)
    .bind(until_seq)
    .bind(session_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    persist_event(
        &mut tx,
        session_id,
        SandboxEventType::RollingSummaryApplied,
        json!({ "until_seq": until_seq, "summary_tokens": estimate_tokens(&summary_text) }),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(until_seq))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, name: &str, goal: Option<&str>) -> AgentCard {
        AgentCard {
            id: id.to_string(),
            name: name.to_string(),
            role_type: "support".to_string(),
            appearance: None,
            personality: None,
            hidden_personality: None,
            biography: None,
            current_goal: goal.map(str::to_string),
            speaking_style: None,
        }
    }

    #[test]
    fn actor_cards_brief_includes_id_and_name() {
        let cards = vec![
            card(
                "0123456789abcdef0123456789abcdef",
                "角色A",
                Some("找回失物"),
            ),
            card("fedcba9876543210fedcba9876543210", "角色B", None),
        ];

        let text = actor_cards_brief(&cards);

        assert_eq!(
            text,
            "- id=0123456789abcdef0123456789abcdef｜角色A（support）：找回失物\n- id=fedcba9876543210fedcba9876543210｜角色B（support）：暂无明确目标"
        );
    }
}
