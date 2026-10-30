// src/services/sandbox/engine.rs — 沙盒主循环与收束（§8.1 / §8.5）
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;
use tokio::sync::{broadcast, OwnedSemaphorePermit};
use tokio_util::sync::CancellationToken;

use crate::config::AppConfig;
use crate::db;
use crate::error::AppError;
use crate::llm::budget::Budget;
use crate::llm::failover::ProviderChain;
use crate::llm::service::LlmService;
use crate::llm::{Message, Purpose};
use crate::models::sandbox::{
    DirectorMode, SandboxEventType, ScriptedPlan, SessionRow, SessionStatus,
};
use crate::services::sandbox::actor;
use crate::services::sandbox::context;
use crate::services::sandbox::director::{self, DirectorInput};
use crate::services::sandbox::events::{now_secs, persist_event, SandboxEvent};
use crate::services::sandbox::manager::load_row;
use crate::services::util::db_err;

#[allow(clippy::too_many_arguments)]
pub async fn run_engine(
    pool: SqlitePool,
    session_id: String,
    config: Arc<ArcSwap<AppConfig>>,
    chain: ProviderChain,
    hub: broadcast::Sender<SandboxEvent>,
    cancel: CancellationToken,
    input_notify: Arc<tokio::sync::Notify>,
    single_step: bool,
    _permit: OwnedSemaphorePermit,
) -> Result<(), AppError> {
    let cfg = config.load().clone();
    let llm = LlmService::new(chain, pool.clone(), Some(session_id.clone()));
    let mut actor_errors = 0u32;
    let mut last_actor: Option<String> = None;
    let mut consecutive = 0u32;

    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        let row = load_row(&pool, &session_id).await?;
        let status = SessionStatus::from_db(&row.status)
            .ok_or_else(|| AppError::internal("非法会话状态"))?;
        if status.is_terminal() || status == SessionStatus::WaitingInput {
            return Ok(());
        }
        if status == SessionStatus::Paused && !single_step {
            return Ok(());
        }
        if status != SessionStatus::Running && status != SessionStatus::Paused {
            return Ok(());
        }
        if row.current_step >= row.step_limit {
            return finalize(&pool, &session_id, &cfg, &llm, &hub, "step_limit", &row).await;
        }
        if row
            .session_deadline
            .is_some_and(|deadline| now_secs() >= deadline)
        {
            return finalize(
                &pool,
                &session_id,
                &cfg,
                &llm,
                &hub,
                "session_timeout",
                &row,
            )
            .await;
        }

        let actor_ids: Vec<String> = serde_json::from_str(&row.actor_ids).unwrap_or_default();
        if actor_ids.is_empty() {
            return fail(&pool, &session_id, &hub, "NO_ACTORS", "会话没有可用角色").await;
        }
        let cards = context::load_agent_cards(&pool, &actor_ids).await?;
        let history =
            context::load_history_window(&pool, &session_id, row.summary_until_seq, 5500).await?;
        let emotion = context::emotion_matrix(&pool, &row.novel_id, &actor_ids).await?;
        let rolling = row.rolling_summary.clone().unwrap_or_default();
        let estimated = context::estimate_tokens(&row.scene_prompt)
            + context::estimate_tokens(&emotion)
            + context::estimate_tokens(&rolling)
            + cards
                .iter()
                .map(|c| context::estimate_tokens(&context::self_card_text(c)))
                .sum::<usize>()
            + history
                .iter()
                .map(|s| context::estimate_tokens(&s.content) + 8)
                .sum::<usize>()
            + 600;
        if estimated > cfg.sandbox.context_hard_cap_tokens as usize {
            return fail(
                &pool,
                &session_id,
                &hub,
                "CONTEXT_OVERFLOW",
                "上下文超过硬顶且无法压缩",
            )
            .await;
        }
        let scripted: Option<ScriptedPlan> = row
            .director_config
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok());
        let mode = DirectorMode::from_db(&row.mode).unwrap_or(DirectorMode::Roundrobin);

        let remaining_session = row
            .session_deadline
            .map(|d| (d - now_secs()).max(1) as u64)
            .unwrap_or(cfg.sandbox.session_timeout_seconds as u64);
        let budget = Budget::with_deadlines(
            Instant::now() + Duration::from_secs(remaining_session),
            Instant::now() + Duration::from_secs(cfg.sandbox.step_timeout_seconds as u64),
            Duration::from_secs(cfg.llm.attempt_timeout_seconds as u64),
        );

        let input = DirectorInput {
            mode,
            actor_ids: &actor_ids,
            current_step: row.current_step.max(0) as u32,
            step_limit: row.step_limit.max(1) as u32,
            scripted: scripted.as_ref(),
            cards: &cards,
            history: &history,
            rolling_summary: &rolling,
            emotion_matrix: &emotion,
            scene: &row.scene_prompt,
            last_actor: last_actor.as_deref(),
            consecutive,
        };
        let decision = director::decide(&input, &cfg, &llm, &budget).await;
        let step = (row.current_step.max(0) + 1) as u32;

        if decision.fallback {
            emit(
                &pool,
                &session_id,
                &hub,
                SandboxEventType::DirectorFallback,
                json!({ "step": step, "fallback_to": "roundrobin", "cause": decision.fallback_reason }),
            )
            .await?;
        } else {
            emit(
                &pool,
                &session_id,
                &hub,
                SandboxEventType::DirectorDecision,
                director::decision_event_payload(&decision, step),
            )
            .await?;
        }

        if let Some((question, suggestions)) = decision.request_input {
            let applied = transition_with_event(
                &pool,
                &session_id,
                &["running", "paused"],
                "waiting_input",
                SandboxEventType::WaitingInput,
                json!({ "step": step, "question": question, "suggestions": suggestions }),
                &hub,
            )
            .await?;
            if !applied {
                return Ok(());
            }
            let wait_timeout =
                Duration::from_secs(cfg.sandbox.waiting_input_timeout_seconds.max(1) as u64);
            loop {
                let notified = tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    _ = input_notify.notified() => true,
                    _ = tokio::time::sleep(wait_timeout) => false,
                };
                if !notified {
                    return finalize(
                        &pool,
                        &session_id,
                        &cfg,
                        &llm,
                        &hub,
                        "waiting_timeout",
                        &row,
                    )
                    .await;
                }
                let refreshed = load_row(&pool, &session_id).await?;
                match SessionStatus::from_db(&refreshed.status) {
                    Some(SessionStatus::Running) => break,
                    Some(SessionStatus::WaitingInput) => continue,
                    _ => return Ok(()),
                }
            }
            continue;
        }
        if decision.terminate {
            let cause = if decision.scripted_done {
                "scripted_done"
            } else {
                "terminate"
            };
            return finalize(&pool, &session_id, &cfg, &llm, &hub, cause, &row).await;
        }

        emit(
            &pool,
            &session_id,
            &hub,
            SandboxEventType::StepStarted,
            json!({ "step": step, "actor_id": decision.actor_id }),
        )
        .await?;

        let directive = row
            .pending_directive
            .clone()
            .or(decision.hint.clone())
            .or_else(|| row.user_directive.clone())
            .unwrap_or_else(|| "继续推进剧情".to_string());

        let actor_future = actor::execute(
            &cards,
            &decision.actor_id,
            step,
            &row.scene_prompt,
            row.user_directive.as_deref().unwrap_or(""),
            &rolling,
            &history,
            &emotion,
            &directive,
            &llm,
            &budget,
            &cfg,
        );
        let outcome = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            result = actor_future => result,
        };

        match outcome {
            Ok(outcome) => {
                commit_step(&pool, &session_id, &hub, &row, &actor_ids, &outcome).await?;
                consecutive = if last_actor.as_deref() == Some(outcome.actor_id.as_str()) {
                    consecutive + 1
                } else {
                    1
                };
                last_actor = Some(outcome.actor_id.clone());
                let refreshed = load_row(&pool, &session_id).await?;
                let _ =
                    context::maybe_compress(&pool, &session_id, &refreshed, &llm, &budget, &cfg)
                        .await?;
            }
            Err(e) => {
                actor_errors += 1;
                emit(
                    &pool,
                    &session_id,
                    &hub,
                    SandboxEventType::ActorError,
                    json!({
                        "step": step,
                        "actor_id": decision.actor_id,
                        "attempt": actor_errors,
                        "error_kind": "actor_error",
                        "message": e.message.chars().take(200).collect::<String>(),
                    }),
                )
                .await?;
                if actor_errors >= 3 {
                    return fail(
                        &pool,
                        &session_id,
                        &hub,
                        "ACTOR_ERRORS",
                        "单角色连续失败达到 3 次",
                    )
                    .await;
                }
            }
        }

        if single_step {
            emit(
                &pool,
                &session_id,
                &hub,
                SandboxEventType::SessionPaused,
                json!({ "by": "step", "step": step }),
            )
            .await?;
            return Ok(());
        }
        let after = load_row(&pool, &session_id).await?;
        let after_status = SessionStatus::from_db(&after.status)
            .ok_or_else(|| AppError::internal("非法会话状态"))?;
        if after_status != SessionStatus::Running {
            return Ok(());
        }
    }
}

async fn commit_step(
    pool: &SqlitePool,
    session_id: &str,
    hub: &broadcast::Sender<SandboxEvent>,
    row: &SessionRow,
    actor_ids: &[String],
    outcome: &actor::ActorOutcome,
) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;

    if let Some(target) = &outcome.target_actor_id {
        if target != &outcome.actor_id && actor_ids.iter().any(|id| id == target) {
            let (a, b) = if outcome.actor_id < *target {
                (outcome.actor_id.clone(), target.clone())
            } else {
                (target.clone(), outcome.actor_id.clone())
            };
            let affected = sqlx::query(
                "UPDATE character_relations SET
                   affinity = MAX(-100, MIN(100, affinity + ?)),
                   trust = MAX(0, MIN(100, trust + ?)),
                   updated_at = strftime('%s','now')
                 WHERE novel_id = ? AND char_a_id = ? AND char_b_id = ?",
            )
            .bind(outcome.affinity_delta)
            .bind(outcome.trust_delta)
            .bind(&row.novel_id)
            .bind(&a)
            .bind(&b)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
            if affected == 0 {
                sqlx::query(
                    "INSERT INTO character_relations(novel_id, char_a_id, char_b_id, affinity, trust) VALUES(?, ?, ?, ?, ?)",
                )
                .bind(&row.novel_id)
                .bind(&a)
                .bind(&b)
                .bind(outcome.affinity_delta.clamp(-100, 100))
                .bind(outcome.trust_delta.clamp(0, 100))
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
            persist_event(
                &mut tx,
                session_id,
                SandboxEventType::EmotionUpdated,
                json!({
                    "step": outcome.step,
                    "pair": [a, b],
                    "affinity_delta": outcome.affinity_delta,
                    "trust_delta": outcome.trust_delta,
                }),
            )
            .await?;
        }
    }

    let action_event = persist_event(
        &mut tx,
        session_id,
        SandboxEventType::ActorAction,
        json!({
            "step": outcome.step,
            "actor_id": outcome.actor_id,
            "actor_name": outcome.actor_name,
            "action": { "type": outcome.action_type, "content": outcome.content },
            "emotion_delta": {
                "target_actor_id": outcome.target_actor_id,
                "affinity_delta": outcome.affinity_delta,
                "trust_delta": outcome.trust_delta,
            },
            "director_note": outcome.director_note,
        }),
    )
    .await?;

    sqlx::query(
        "UPDATE sandbox_sessions SET current_step = ?, pending_directive = NULL, updated_at = strftime('%s','now') WHERE id = ?",
    )
    .bind(outcome.step as i64)
    .bind(session_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    let _ = hub.send(action_event);
    Ok(())
}

async fn finalize(
    pool: &SqlitePool,
    session_id: &str,
    cfg: &AppConfig,
    llm: &LlmService,
    hub: &broadcast::Sender<SandboxEvent>,
    cause: &str,
    row: &SessionRow,
) -> Result<(), AppError> {
    let applied = transition_with_event(
        pool,
        session_id,
        &["running", "paused", "waiting_input", "finalizing"],
        "finalizing",
        SandboxEventType::FinalizeStarted,
        json!({ "step": row.current_step, "cause": cause }),
        hub,
    )
    .await?;
    if !applied {
        return Ok(());
    }

    let history = context::load_history(pool, session_id, 0, 10000).await?;
    let chapter_context = match &row.chapter_id {
        Some(cid) => {
            let title: Option<String> =
                sqlx::query_scalar("SELECT title FROM chapters WHERE id = ?")
                    .bind(cid)
                    .fetch_optional(pool)
                    .await
                    .map_err(db_err)?;
            title.unwrap_or_default()
        }
        None => "（无关联章节）".to_string(),
    };
    let ctx = json!({
        "chapter_context": chapter_context,
        "rolling_summary": row.rolling_summary.clone().unwrap_or_default(),
        "steps": history,
        "user_directive": row.user_directive.clone().unwrap_or_default(),
    });
    let prompt = crate::llm::prompts::render(crate::llm::prompts::OUTLINE_FINALIZE, &ctx)?;
    let budget = Budget::new(
        cfg.sandbox.session_timeout_seconds as u64,
        cfg.sandbox.step_timeout_seconds as u64,
        cfg.llm.attempt_timeout_seconds as u64,
    );
    let outline = match llm
        .complete_text(
            &budget,
            Purpose::Finalize,
            vec![Message::user(prompt)],
            1800,
        )
        .await
    {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(error = %e, "大纲收束降级");
            let steps = history
                .iter()
                .map(|s| format!("- 第{}步 {}：{}", s.no, s.actor_name, s.content))
                .collect::<Vec<_>>()
                .join("\n");
            format!("⚠️ 降级模式（LLM 不可用）\n\n## 剧情梗概\n{steps}")
        }
    };

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    if let Some(cid) = &row.chapter_id {
        sqlx::query(
            "UPDATE chapters SET final_outline = ?, updated_at = strftime('%s','now') WHERE id = ?",
        )
        .bind(&outline)
        .bind(cid)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    let completed = sqlx::query(
        "UPDATE sandbox_sessions SET summary_text = ?, status = 'completed', finished_at = strftime('%s','now'), updated_at = strftime('%s','now') WHERE id = ? AND status = 'finalizing'",
    )
    .bind(&outline)
    .bind(session_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if completed == 0 {
        tx.rollback().await.map_err(db_err)?;
        return Ok(());
    }
    if row.chapter_id.is_some() {
        persist_event(
            &mut tx,
            session_id,
            SandboxEventType::OutlineReady,
            json!({ "chapter_id": row.chapter_id, "outline_markdown": outline }),
        )
        .await?;
    }
    let done = persist_event(
        &mut tx,
        session_id,
        SandboxEventType::SessionCompleted,
        json!({ "step": row.current_step }),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    let _ = hub.send(done);
    Ok(())
}

async fn fail(
    pool: &SqlitePool,
    session_id: &str,
    hub: &broadcast::Sender<SandboxEvent>,
    code: &str,
    message: &str,
) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let affected = sqlx::query(
        "UPDATE sandbox_sessions SET status = 'failed', error_code = ?, error_message = ?, finished_at = strftime('%s','now'), updated_at = strftime('%s','now') WHERE id = ? AND status IN ('running','paused','waiting_input','finalizing')",
    )
    .bind(code)
    .bind(message)
    .bind(session_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if affected == 0 {
        tx.rollback().await.map_err(db_err)?;
        return Ok(());
    }
    let event = persist_event(
        &mut tx,
        session_id,
        SandboxEventType::SessionFailed,
        json!({ "error_code": code, "message": message }),
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    let _ = hub.send(event);
    Ok(())
}

async fn transition_with_event(
    pool: &SqlitePool,
    session_id: &str,
    from: &[&str],
    to: &str,
    event_type: SandboxEventType,
    payload: Value,
    hub: &broadcast::Sender<SandboxEvent>,
) -> Result<bool, AppError> {
    let placeholders = vec!["?"; from.len()].join(",");
    let sql = format!(
        "UPDATE sandbox_sessions SET status = ?, updated_at = strftime('%s','now') WHERE id = ? AND status IN ({placeholders})"
    );
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let mut stmt = sqlx::query(&sql).bind(to).bind(session_id);
    for status in from {
        stmt = stmt.bind(*status);
    }
    let affected = stmt
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
    if affected == 0 {
        tx.rollback().await.map_err(db_err)?;
        return Ok(false);
    }
    let event = persist_event(&mut tx, session_id, event_type, payload).await?;
    tx.commit().await.map_err(db_err)?;
    let _ = hub.send(event);
    Ok(true)
}

async fn emit(
    pool: &SqlitePool,
    session_id: &str,
    hub: &broadcast::Sender<SandboxEvent>,
    event_type: SandboxEventType,
    payload: Value,
) -> Result<(), AppError> {
    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let event = persist_event(&mut tx, session_id, event_type, payload).await?;
    tx.commit().await.map_err(db_err)?;
    let _ = hub.send(event);
    Ok(())
}
