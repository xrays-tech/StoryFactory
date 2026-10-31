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

type NovelRow = (String, Option<String>, Option<String>, Option<String>);

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

/// 导演所需的全局剧情上下文：作品设定/卷章定位 + 上一场景 + 本章大纲。
#[derive(Debug, Clone, Default, Serialize)]
pub struct StoryContext {
    pub global_plot: String,
    pub previous_scene: String,
    pub chapter_outline: String,
}

#[derive(Debug, Clone, Default)]
pub struct NovelContext {
    pub title: String,
    pub world_view: Option<String>,
    pub background: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VolumeContext {
    pub title: String,
    pub summary: Option<String>,
    pub volume_index: i64,
}

#[derive(Debug, Clone)]
pub struct ChapterContext {
    pub title: String,
    pub final_outline: Option<String>,
    pub chapter_index: i64,
}

/// 按字符边界截断；超长追加省略号。
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// 汇总级硬顶：结果字符数（含省略号）不超过 max。
fn truncate_total(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn clip(value: Option<&str>, max: usize) -> Option<String> {
    let text = value?.trim();
    if text.is_empty() {
        return None;
    }
    Some(truncate_chars(text, max))
}

const BEATS_MAX: usize = 12;
const BEAT_MAX_CHARS: usize = 80;

/// 单条情节裁剪到 80 字以内（含省略号）。
fn clip_beat(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= BEAT_MAX_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(BEAT_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

fn json_string_array(raw: &str) -> Option<Vec<String>> {
    let values: Vec<String> = serde_json::from_str(raw).ok()?;
    Some(
        values
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

/// 返回 `## 标题` 小节的正文行（不含标题本身）。
fn markdown_section<'a>(outline: &'a str, heading: &str) -> Vec<&'a str> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in outline.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("## ") {
            if in_section {
                break;
            }
            in_section = title.trim() == heading;
            continue;
        }
        if in_section {
            lines.push(line);
        }
    }
    lines
}

/// 解析列表项：`- `、`* ` 或 `1.` / `1、` / `1)` 等数字列表。
fn list_item_text(line: &str) -> Option<&str> {
    let text = line.trim();
    for prefix in ["- ", "* "] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let rest = rest.trim();
            return (!rest.is_empty()).then_some(rest);
        }
    }
    let digits = text.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = &text[digits..];
    let rest = rest
        .strip_prefix('.')
        .or_else(|| rest.strip_prefix('、'))
        .or_else(|| rest.strip_prefix(')'))
        .or_else(|| rest.strip_prefix('）'))?;
    let rest = rest.trim();
    (!rest.is_empty()).then_some(rest)
}

fn beats_from_list(outline: &str, heading: &str) -> Vec<String> {
    markdown_section(outline, heading)
        .into_iter()
        .filter_map(list_item_text)
        .map(clip_beat)
        .filter(|item| !item.is_empty())
        .take(BEATS_MAX)
        .collect()
}

fn beats_from_prose(outline: &str, heading: &str) -> Vec<String> {
    let prose = markdown_section(outline, heading).join("");
    prose
        .split(['。', '；', ';'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(clip_beat)
        .take(BEATS_MAX)
        .collect()
}

/// 解析本章情节清单：优先使用显式 JSON 数组，否则从大纲 Markdown 派生。
pub fn parse_beats(outline: &str, beats_json: Option<&str>) -> Vec<String> {
    if let Some(beats) = beats_json.and_then(json_string_array) {
        if !beats.is_empty() {
            return beats;
        }
    }
    let listed = beats_from_list(outline, "关键场景");
    if !listed.is_empty() {
        return listed;
    }
    beats_from_prose(outline, "剧情梗概")
}

/// 已消费的下标（去重、保持出现顺序）。
pub fn parse_beats_done(raw: Option<&str>) -> Vec<usize> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let Ok(values) = serde_json::from_str::<Vec<usize>>(raw) else {
        return Vec::new();
    };
    let mut done = Vec::with_capacity(values.len());
    for value in values {
        if !done.contains(&value) {
            done.push(value);
        }
    }
    done
}

/// 尚未演绎的情节（保持原始下标）。
pub fn remaining_beats<'a>(beats: &'a [String], done: &[usize]) -> Vec<(usize, &'a str)> {
    beats
        .iter()
        .enumerate()
        .filter(|(index, _)| !done.contains(index))
        .map(|(index, beat)| (index, beat.as_str()))
        .collect()
}

/// 读取章节情节清单；章节缺失时返回空清单。
pub async fn load_chapter_beats(
    pool: &SqlitePool,
    chapter_id: &str,
) -> Result<Vec<String>, AppError> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT final_outline, beats FROM chapters WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(chapter_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(row
        .map(|(outline, beats)| {
            parse_beats(outline.as_deref().unwrap_or_default(), beats.as_deref())
        })
        .unwrap_or_default())
}

/// 标题是否已自带「第X卷/章/回/节」序号，避免渲染时出现「第2卷《第二卷 破晓》」。
fn has_ordinal(title: &str) -> bool {
    let Some(rest) = title.trim().strip_prefix('第') else {
        return false;
    };
    let mut chars = rest.chars().peekable();
    let mut consumed = 0usize;
    while let Some(c) = chars.peek() {
        let numeric = c.is_ascii_digit()
            || matches!(
                c,
                '零' | '〇'
                    | '一'
                    | '二'
                    | '两'
                    | '三'
                    | '四'
                    | '五'
                    | '六'
                    | '七'
                    | '八'
                    | '九'
                    | '十'
                    | '百'
                    | '千'
            );
        if !numeric {
            break;
        }
        consumed += 1;
        chars.next();
    }
    consumed > 0 && matches!(chars.peek(), Some('卷' | '章' | '回' | '节'))
}

/// 去掉标题自带的「第X卷/章/回/节」及其后的分隔符，供「第N卷《…》」复用。
fn strip_ordinal_prefix(title: &str) -> &str {
    let title = title.trim();
    if !has_ordinal(title) {
        return title;
    }
    let Some(rest) = title.strip_prefix('第') else {
        return title;
    };
    let body = rest.trim_start_matches(|c: char| {
        c.is_ascii_digit() || "零〇一二两三四五六七八九十百千".contains(c)
    });
    let Some(unit) = body.chars().next() else {
        return title;
    };
    if !matches!(unit, '卷' | '章' | '回' | '节') {
        return title;
    }
    body[unit.len_utf8()..]
        .trim_start_matches(|c: char| c.is_whitespace() || "、:：·-—《》".contains(c))
        .trim()
}

/// 列表项标签：标题已带序号时原样展示，避免与索引重复。
fn list_label(index: i64, title: &str, unit: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return format!("第{index}{unit}");
    }
    if has_ordinal(title) {
        return title.to_string();
    }
    format!("第{index}{unit} {title}")
}

/// 纯函数：渲染作品设定与卷章定位，总长 ≤2000 字符。
pub fn render_global_plot(
    novel: &NovelContext,
    chapter: Option<&ChapterContext>,
    volume: Option<&VolumeContext>,
    chapter_list: &[(i64, String)],
) -> String {
    let mut out = String::new();
    out.push_str("# 作品设定\n");
    out.push_str(&format!("书名：{}\n", novel.title.trim()));
    if let Some(world_view) = clip(novel.world_view.as_deref(), 800) {
        out.push_str(&format!("世界观：{world_view}\n"));
    }
    if let Some(background) = clip(novel.background.as_deref(), 600) {
        out.push_str(&format!("背景：{background}\n"));
    }
    if let Some(summary) = clip(novel.summary.as_deref(), 600) {
        out.push_str(&format!("简介：{summary}\n"));
    }

    if let (Some(chapter), Some(volume)) = (chapter, volume) {
        out.push_str("\n# 卷章定位\n");
        out.push_str(&format!(
            "卷：第{}卷《{}》\n",
            volume.volume_index,
            strip_ordinal_prefix(&volume.title)
        ));
        if let Some(summary) = clip(volume.summary.as_deref(), 400) {
            out.push_str(&format!("卷摘要：{summary}\n"));
        }
        out.push_str(&format!(
            "本章：第{}章《{}》\n",
            chapter.chapter_index,
            strip_ordinal_prefix(&chapter.title)
        ));
        if let Some(outline) = clip(chapter.final_outline.as_deref(), 800) {
            out.push_str(&format!("本章既有大纲：{outline}\n"));
        }
        if !chapter_list.is_empty() {
            out.push_str("章节目录：\n");
            for (index, title) in chapter_list {
                let marker = if *index == chapter.chapter_index {
                    " ← 当前章"
                } else {
                    ""
                };
                out.push_str(&format!("- {}{marker}\n", list_label(*index, title, "章")));
            }
        }
    }

    truncate_total(out.trim_end(), 2000)
}

/// 纯函数：上一场景优先用上一章定稿大纲，其次用同书最近完成会话的摘要。
pub fn render_previous_scene(
    previous_outline: Option<&str>,
    prior_session_summary: Option<&str>,
) -> String {
    for text in [previous_outline, prior_session_summary]
        .into_iter()
        .flatten()
    {
        let text = text.trim();
        if !text.is_empty() {
            return truncate_total(text, 800);
        }
    }
    "（无前置场景，本场景为起点）".to_string()
}

pub async fn load_story_context(
    pool: &SqlitePool,
    novel_id: &str,
    chapter_id: Option<&str>,
    session_id: &str,
) -> Result<StoryContext, AppError> {
    let novel: Option<NovelRow> =
        sqlx::query_as("SELECT title, world_view, background, summary FROM novels WHERE id = ?")
            .bind(novel_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let Some((title, world_view, background, summary)) = novel else {
        return Err(AppError::internal("作品不存在，无法加载剧情上下文"));
    };
    let novel = NovelContext {
        title,
        world_view,
        background,
        summary,
    };

    let Some(chapter_id) = chapter_id else {
        let prior_summary: Option<String> = sqlx::query_scalar(
            "SELECT summary_text FROM sandbox_sessions
             WHERE novel_id = ? AND id <> ? AND status = 'completed'
               AND summary_text IS NOT NULL AND TRIM(summary_text) <> ''
             ORDER BY COALESCE(finished_at, updated_at) DESC LIMIT 1",
        )
        .bind(novel_id)
        .bind(session_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        return Ok(StoryContext {
            global_plot: render_global_plot(&novel, None, None, &[]),
            previous_scene: render_previous_scene(None, prior_summary.as_deref()),
            chapter_outline: String::new(),
        });
    };

    let chapter: Option<(String, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT title, final_outline, volume_id, chapter_index FROM chapters
         WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(chapter_id)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some((chapter_title, final_outline, volume_id, chapter_index)) = chapter else {
        return Err(AppError::internal("章节不存在，无法加载剧情上下文"));
    };

    let volume: Option<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT title, summary, volume_index FROM volumes
         WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(&volume_id)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some((volume_title, volume_summary, volume_index)) = volume else {
        return Err(AppError::internal("卷不存在，无法加载剧情上下文"));
    };
    let volume_ctx = VolumeContext {
        title: volume_title,
        summary: volume_summary,
        volume_index,
    };
    let chapter_outline = final_outline.clone().unwrap_or_default();
    let chapter_ctx = ChapterContext {
        title: chapter_title,
        final_outline,
        chapter_index,
    };

    let chapter_list: Vec<(i64, String)> = sqlx::query_as(
        "SELECT chapter_index, title FROM chapters
         WHERE volume_id = ? AND deleted_at IS NULL ORDER BY chapter_index",
    )
    .bind(&volume_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let previous_outline: Option<String> = sqlx::query_scalar(
        "SELECT final_outline FROM chapters
         WHERE volume_id = ? AND chapter_index < ? AND deleted_at IS NULL
           AND final_outline IS NOT NULL AND TRIM(final_outline) <> ''
         ORDER BY chapter_index DESC LIMIT 1",
    )
    .bind(&volume_id)
    .bind(chapter_index)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let previous_outline = match previous_outline {
        Some(outline) => Some(outline),
        None => sqlx::query_scalar(
            "SELECT c.final_outline FROM chapters c
                 JOIN volumes v ON v.id = c.volume_id
                 WHERE v.novel_id = ? AND v.volume_index < ?
                   AND v.deleted_at IS NULL AND c.deleted_at IS NULL
                   AND c.final_outline IS NOT NULL AND TRIM(c.final_outline) <> ''
                 ORDER BY v.volume_index DESC, c.chapter_index DESC LIMIT 1",
        )
        .bind(novel_id)
        .bind(volume_index)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?,
    };

    Ok(StoryContext {
        global_plot: render_global_plot(
            &novel,
            Some(&chapter_ctx),
            Some(&volume_ctx),
            &chapter_list,
        ),
        previous_scene: render_previous_scene(previous_outline.as_deref(), None),
        chapter_outline,
    })
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

    fn sample_novel() -> NovelContext {
        NovelContext {
            title: "星海回声".to_string(),
            world_view: Some("星际帝国与失落舰队对峙".to_string()),
            background: Some("边境星域战云密布".to_string()),
            summary: Some("一名信使寻找失踪的指挥官".to_string()),
        }
    }

    #[test]
    fn render_global_plot_includes_novel_volume_and_current_chapter() {
        let novel = sample_novel();
        let volume = VolumeContext {
            title: "破晓".to_string(),
            summary: Some("主角重返边境".to_string()),
            volume_index: 2,
        };
        let chapter = ChapterContext {
            title: "归途".to_string(),
            final_outline: Some("本章既有大纲：穿越封锁线".to_string()),
            chapter_index: 2,
        };
        let list = vec![(1, "启程".to_string()), (2, "归途".to_string())];

        let text = render_global_plot(&novel, Some(&chapter), Some(&volume), &list);

        assert!(text.contains("星海回声"), "缺少书名: {text}");
        assert!(
            text.contains("星际帝国与失落舰队对峙"),
            "缺少世界观: {text}"
        );
        assert!(text.contains("第2卷《破晓》"), "缺少卷定位: {text}");
        assert!(text.contains("本章：第2章《归途》"), "缺少章定位: {text}");
        assert!(
            text.contains("- 第2章 归途 ← 当前章"),
            "未标出当前章: {text}"
        );
        assert_eq!(
            text.matches("← 当前章").count(),
            1,
            "当前章标记应唯一: {text}"
        );
    }

    #[test]
    fn render_global_plot_strips_duplicate_ordinals() {
        let novel = sample_novel();
        let volume = VolumeContext {
            title: "第二卷 破晓".to_string(),
            summary: None,
            volume_index: 2,
        };
        let chapter = ChapterContext {
            title: "第四章 归途".to_string(),
            final_outline: None,
            chapter_index: 2,
        };
        let list = vec![
            (1, "第三章 封锁".to_string()),
            (2, "第四章 归途".to_string()),
        ];

        let text = render_global_plot(&novel, Some(&chapter), Some(&volume), &list);

        assert!(text.contains("卷：第2卷《破晓》"), "卷标题重复序号: {text}");
        assert!(
            text.contains("本章：第2章《归途》"),
            "章标题重复序号: {text}"
        );
        assert!(
            text.contains("- 第四章 归途 ← 当前章"),
            "列表项标题应原样展示: {text}"
        );
        assert!(!text.contains("第2卷《第二卷"), "出现重复卷序号: {text}");
    }

    #[test]
    fn render_previous_scene_prefers_outline_then_session_summary() {
        assert_eq!(
            render_previous_scene(Some("上一章：他们在码头分手"), Some("更旧的摘要")),
            "上一章：他们在码头分手"
        );
        assert_eq!(
            render_previous_scene(None, Some("上一场：他们在码头分手")),
            "上一场：他们在码头分手"
        );
        assert_eq!(
            render_previous_scene(None, None),
            "（无前置场景，本场景为起点）"
        );
        assert_eq!(
            render_previous_scene(Some("   "), Some("")),
            "（无前置场景，本场景为起点）"
        );
    }

    #[test]
    fn truncate_chars_respects_boundary_and_empty_input() {
        let exact = "字".repeat(800);
        assert_eq!(truncate_chars(&exact, 800), exact, "恰好 800 字不应截断");
        assert_eq!(truncate_chars("", 800), "");

        let over = "字".repeat(801);
        let clipped = truncate_chars(&over, 800);
        assert_eq!(clipped.chars().count(), 801, "800 字 + 省略号");
        assert!(clipped.ends_with('…'));
        assert_eq!(clipped.chars().take(800).collect::<String>(), exact);
    }

    #[test]
    fn render_previous_scene_truncates_long_outline() {
        let text = render_previous_scene(Some(&"雨".repeat(900)), None);
        assert_eq!(text.chars().count(), 800, "汇总级上限含省略号");
        assert!(text.ends_with('…'));
    }

    #[test]
    fn render_global_plot_omits_blank_fields_and_caps_total_length() {
        let blank = NovelContext {
            title: "空字段".to_string(),
            world_view: Some("  ".to_string()),
            background: None,
            summary: Some(String::new()),
        };
        let text = render_global_plot(&blank, None, None, &[]);
        assert!(!text.contains("世界观："), "空白字段不应渲染: {text}");
        assert!(!text.contains("背景："), "缺失字段不应渲染: {text}");
        assert!(!text.contains("简介："), "空字符串字段不应渲染: {text}");

        let novel = NovelContext {
            title: "超长作品".to_string(),
            world_view: Some("界".repeat(800)),
            background: Some("背".repeat(600)),
            summary: Some("简".repeat(600)),
        };
        let volume = VolumeContext {
            title: "长卷".to_string(),
            summary: Some("卷".repeat(400)),
            volume_index: 1,
        };
        let chapter = ChapterContext {
            title: "长章".to_string(),
            final_outline: Some("纲".repeat(800)),
            chapter_index: 1,
        };
        let text = render_global_plot(&novel, Some(&chapter), Some(&volume), &[]);
        assert!(
            text.chars().count() <= 2000,
            "global_plot 总长必须 ≤2000，实际 {}",
            text.chars().count()
        );
    }
}
