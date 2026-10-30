// src/services/sandbox/manager.rs — 会话注册表、并发闸、创建/快照/控制编排（§8.4）
use std::sync::Arc;

use arc_swap::ArcSwap;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;
use tokio::sync::{broadcast, Notify, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::config::AppConfig;
use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::llm::factory;
use crate::llm::failover::ProviderChain;
use crate::models::common::{resolve_page, Page};
use crate::models::sandbox::{
    ActorName, CreateSessionReq, DirectorMode, PurposeCount, SandboxEventType, SessionRow,
    SessionSnapshot, SessionStatus, UsageSummary,
};
use crate::services::sandbox::engine;
use crate::services::sandbox::events::{now_secs, persist_event, SandboxEvent};
use crate::services::util::{db_err, invalid, not_found, novel_alive};

#[derive(Clone)]
pub struct SessionHandle {
    pub cancel: CancellationToken,
    pub hub: broadcast::Sender<SandboxEvent>,
    pub input_notify: Arc<Notify>,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartKind {
    Start,
    Resume,
    Recover,
    Step,
}

pub struct SessionManager {
    registry: Arc<DashMap<String, SessionHandle>>,
    semaphore: Arc<Semaphore>,
    pool: SqlitePool,
    config: Arc<ArcSwap<AppConfig>>,
    chain_override: Option<ProviderChain>,
}

impl SessionManager {
    pub fn new(pool: SqlitePool, config: Arc<ArcSwap<AppConfig>>, max_concurrent: usize) -> Self {
        Self {
            registry: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrent.max(1))),
            pool,
            config,
            chain_override: None,
        }
    }

    pub fn with_chain(
        pool: SqlitePool,
        config: Arc<ArcSwap<AppConfig>>,
        max_concurrent: usize,
        chain: ProviderChain,
    ) -> Self {
        let mut manager = Self::new(pool, config, max_concurrent);
        manager.chain_override = Some(chain);
        manager
    }

    pub fn subscribe(&self, session_id: &str) -> Option<broadcast::Receiver<SandboxEvent>> {
        self.registry
            .get(session_id)
            .map(|handle| handle.hub.subscribe())
    }

    pub fn cancel(&self, session_id: &str) {
        if let Some(handle) = self.registry.get(session_id) {
            handle.cancel.cancel();
        }
    }

    pub fn notify_input(&self, session_id: &str) {
        if let Some(handle) = self.registry.get(session_id) {
            handle.input_notify.notify_one();
        }
    }

    fn build_chain(&self) -> ProviderChain {
        if let Some(chain) = &self.chain_override {
            return chain.clone();
        }
        factory::build_chain(&self.config.load())
    }

    fn reserve_handle(&self, session_id: &str) -> Result<SessionHandle, AppError> {
        let (hub, _) = broadcast::channel(256);
        let handle = SessionHandle {
            cancel: CancellationToken::new(),
            hub,
            input_notify: Arc::new(Notify::new()),
            token: uuid::Uuid::new_v4().to_string(),
        };
        match self.registry.entry(session_id.to_string()) {
            Entry::Vacant(entry) => {
                entry.insert(handle.clone());
                Ok(handle)
            }
            Entry::Occupied(_) => Err(AppError::new(
                ErrorCode::SessionConflict,
                "会话已有运行中的任务",
            )),
        }
    }

    pub async fn create(
        &self,
        novel_id: &str,
        chapter_id: Option<&str>,
        req: CreateSessionReq,
    ) -> Result<SessionSnapshot, AppError> {
        novel_alive(&self.pool, novel_id).await?;
        let cfg = self.config.load();
        let scene = req.scene_prompt.trim();
        if scene.chars().count() < 10 || scene.chars().count() > 4000 {
            return Err(invalid(vec![(
                "scene_prompt",
                "scene_prompt 长度必须为 10..4000 字符".to_string(),
            )]));
        }
        if req.actor_ids.is_empty() || req.actor_ids.len() > 8 {
            return Err(invalid(vec![(
                "actor_ids",
                "actor_ids 数量必须为 1..8".to_string(),
            )]));
        }
        let mut unique = req.actor_ids.clone();
        unique.sort();
        unique.dedup();
        if unique.len() != req.actor_ids.len() {
            return Err(invalid(vec![(
                "actor_ids",
                "actor_ids 不能重复".to_string(),
            )]));
        }
        let step_limit = req.step_limit.unwrap_or(cfg.sandbox.max_steps).clamp(1, 60);
        if let Some(directive) = &req.user_directive {
            if directive.chars().count() > 500 {
                return Err(invalid(vec![(
                    "user_directive",
                    "user_directive 不能超过 500 字符".to_string(),
                )]));
            }
        }
        if req.mode == DirectorMode::Scripted {
            let plan = req.director_config.as_ref().ok_or_else(|| {
                invalid(vec![(
                    "director_config",
                    "scripted 模式必须提供导演脚本".to_string(),
                )])
            })?;
            if plan.steps.is_empty() || plan.steps.len() > step_limit as usize {
                return Err(invalid(vec![(
                    "director_config",
                    "scripted 步数必须为 1..=step_limit".to_string(),
                )]));
            }
            let allowed: std::collections::HashSet<&str> =
                req.actor_ids.iter().map(String::as_str).collect();
            for step in &plan.steps {
                if !allowed.contains(step.actor_id.as_str()) {
                    return Err(invalid(vec![(
                        "director_config",
                        "脚本 actor_id 必须在 actor_ids 中".to_string(),
                    )]));
                }
            }
        }
        for actor in &req.actor_ids {
            let found: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM characters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
            )
            .bind(actor)
            .bind(novel_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
            if found.is_none() {
                return Err(invalid(vec![(
                    "actor_ids",
                    "所有角色必须属于当前项目且未删除".to_string(),
                )]));
            }
        }
        if let Some(cid) = chapter_id {
            let found: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM chapters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
            )
            .bind(cid)
            .bind(novel_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
            if found.is_none() {
                return Err(not_found(ErrorCode::ChapterNotFound, "章节不存在或已删除"));
            }
        }

        let template_versions = crate::llm::prompts::snapshot_versions(&self.pool).await?;
        let actor_ids_json =
            serde_json::to_string(&req.actor_ids).unwrap_or_else(|_| "[]".to_string());
        let director_config = req
            .director_config
            .as_ref()
            .map(|plan| serde_json::to_string(plan).unwrap_or_else(|_| "{}".to_string()));
        let id = uuid::Uuid::new_v4().to_string().replace('-', "");

        let mut tx = db::begin_immediate(&self.pool).await.map_err(db_err)?;
        sqlx::query(
            "INSERT INTO sandbox_sessions(id, novel_id, chapter_id, scene_prompt, actor_ids, mode, step_limit, director_config, template_versions, user_directive) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(novel_id)
        .bind(chapter_id)
        .bind(scene)
        .bind(&actor_ids_json)
        .bind(req.mode.as_str())
        .bind(step_limit as i64)
        .bind(&director_config)
        .bind(template_versions.to_string())
        .bind(&req.user_directive)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;

        if req.autostart.unwrap_or(true) {
            self.start_session(&id, StartKind::Start).await?;
        }
        self.snapshot(&id).await
    }

    pub async fn snapshot(&self, session_id: &str) -> Result<SessionSnapshot, AppError> {
        let row = load_row(&self.pool, session_id).await?;
        let actor_ids: Vec<String> = serde_json::from_str(&row.actor_ids).unwrap_or_default();
        let mut actor_names = Vec::new();
        for actor_id in &actor_ids {
            let name: Option<String> =
                sqlx::query_scalar("SELECT name FROM characters WHERE id = ?")
                    .bind(actor_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(db_err)?;
            match name {
                Some(name) => actor_names.push(ActorName {
                    id: actor_id.clone(),
                    name,
                    deleted: false,
                }),
                None => actor_names.push(ActorName {
                    id: actor_id.clone(),
                    name: "(已删除角色)".to_string(),
                    deleted: true,
                }),
            }
        }
        let usage = usage_summary(&self.pool, session_id).await?;
        let status = SessionStatus::from_db(&row.status)
            .ok_or_else(|| AppError::internal(format!("非法会话状态: {}", row.status)))?;
        Ok(SessionSnapshot {
            id: row.id,
            novel_id: row.novel_id,
            chapter_id: row.chapter_id,
            status,
            mode: DirectorMode::from_db(&row.mode).unwrap_or(DirectorMode::Roundrobin),
            current_step: row.current_step.max(0) as u32,
            step_limit: row.step_limit.max(1) as u32,
            actor_ids,
            actor_names,
            last_event_seq: row.last_event_seq.max(0) as u64,
            can_recover: status == SessionStatus::Interrupted,
            usage,
            error: row
                .error_code
                .map(|code| crate::models::sandbox::SessionError {
                    code,
                    message: row.error_message.unwrap_or_default(),
                }),
            created_at: row.created_at,
            updated_at: row.updated_at,
            finished_at: row.finished_at,
            session_deadline: row.session_deadline,
        })
    }

    pub async fn list(
        &self,
        novel_id: &str,
        status: Option<SessionStatus>,
        page: i64,
        page_size: i64,
    ) -> Result<Page<SessionSnapshot>, AppError> {
        novel_alive(&self.pool, novel_id).await?;
        let (p, ps, limit, offset) = resolve_page(page, page_size);
        let mut where_sql = String::from("novel_id = ?");
        if status.is_some() {
            where_sql.push_str(" AND status = ?");
        }
        let count_sql = format!("SELECT COUNT(*) FROM sandbox_sessions WHERE {where_sql}");
        let mut count_stmt = sqlx::query_scalar::<_, i64>(&count_sql).bind(novel_id);
        if let Some(s) = status {
            count_stmt = count_stmt.bind(s.as_str());
        }
        let total = count_stmt.fetch_one(&self.pool).await.map_err(db_err)?;
        let ids_sql = format!(
            "SELECT id FROM sandbox_sessions WHERE {where_sql} ORDER BY created_at DESC LIMIT ? OFFSET ?"
        );
        let mut ids_stmt = sqlx::query_scalar::<_, String>(&ids_sql).bind(novel_id);
        if let Some(s) = status {
            ids_stmt = ids_stmt.bind(s.as_str());
        }
        let ids = ids_stmt
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let mut items = Vec::new();
        for id in ids {
            items.push(self.snapshot(&id).await?);
        }
        Ok(Page {
            items,
            total,
            page: p,
            page_size: ps,
        })
    }

    pub async fn start_session(&self, session_id: &str, kind: StartKind) -> Result<(), AppError> {
        let row = load_row(&self.pool, session_id).await?;
        let status = SessionStatus::from_db(&row.status)
            .ok_or_else(|| AppError::internal("非法会话状态"))?;
        match kind {
            StartKind::Start if status != SessionStatus::Created => {
                return Err(AppError::new(
                    ErrorCode::SessionConflict,
                    "只有 created 会话可以 start",
                ));
            }
            StartKind::Resume if status != SessionStatus::Paused => {
                return Err(AppError::new(
                    ErrorCode::SessionConflict,
                    "只有 paused 会话可以 resume",
                ));
            }
            StartKind::Recover if status != SessionStatus::Interrupted => {
                return Err(AppError::new(
                    ErrorCode::SessionConflict,
                    "只有 interrupted 会话可以 recover",
                ));
            }
            StartKind::Step if status != SessionStatus::Paused => {
                return Err(AppError::new(
                    ErrorCode::SessionConflict,
                    "只有 paused 会话可以 step",
                ));
            }
            _ => {}
        }

        let chain = self.build_chain();
        if chain.is_empty() {
            return Err(AppError::new(
                ErrorCode::LlmConfigMissing,
                "未配置 LLM API Key，无法启动沙盒",
            ));
        }
        let permit = self.semaphore.clone().try_acquire_owned().map_err(|_| {
            AppError::new(ErrorCode::RateLimited, "并发沙盒会话已达上限，请稍后重试")
        })?;

        let handle = self.reserve_handle(session_id)?;

        if kind != StartKind::Step {
            let deadline = now_secs() + self.config.load().sandbox.session_timeout_seconds as i64;
            let from = match kind {
                StartKind::Start => "created",
                StartKind::Resume | StartKind::Step => "paused",
                StartKind::Recover => "interrupted",
            };
            let mut tx = db::begin_immediate(&self.pool).await.map_err(db_err)?;
            let affected = sqlx::query(
                "UPDATE sandbox_sessions SET status = 'running', session_deadline = ?, error_code = NULL, error_message = NULL, updated_at = strftime('%s','now') WHERE id = ? AND status = ?",
            )
            .bind(deadline)
            .bind(session_id)
            .bind(from)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
            if affected == 0 {
                tx.rollback().await.map_err(db_err)?;
                self.registry
                    .remove_if(session_id, |_, h| h.token == handle.token);
                return Err(AppError::new(
                    ErrorCode::SessionConflict,
                    "状态已变化，无法启动会话",
                ));
            }
            let actor_ids: Vec<String> = serde_json::from_str(&row.actor_ids).unwrap_or_default();
            let (event_type, payload) = match kind {
                StartKind::Start => (
                    SandboxEventType::SessionStarted,
                    json!({
                        "mode": row.mode,
                        "actor_ids": actor_ids,
                        "step_limit": row.step_limit,
                        "deadline": deadline,
                    }),
                ),
                StartKind::Resume => (
                    SandboxEventType::SessionResumed,
                    json!({ "cause": "resume", "step": row.current_step }),
                ),
                StartKind::Recover => (
                    SandboxEventType::SessionResumed,
                    json!({ "cause": "recover", "step": row.current_step }),
                ),
                StartKind::Step => unreachable!(),
            };
            persist_event(&mut tx, session_id, event_type, payload).await?;
            tx.commit().await.map_err(db_err)?;
        }

        self.spawn_engine(
            session_id.to_string(),
            chain,
            permit,
            kind == StartKind::Step,
            handle,
        );
        Ok(())
    }

    fn spawn_engine(
        &self,
        session_id: String,
        chain: ProviderChain,
        permit: OwnedSemaphorePermit,
        single_step: bool,
        handle: SessionHandle,
    ) {
        let pool = self.pool.clone();
        let config = self.config.clone();
        let registry = self.registry.clone();
        let token = handle.token.clone();
        let cancel = handle.cancel.clone();
        let hub = handle.hub.clone();
        let input_notify = handle.input_notify.clone();
        tokio::spawn(async move {
            let result = engine::run_engine(
                pool,
                session_id.clone(),
                config,
                chain,
                hub,
                cancel,
                input_notify,
                single_step,
                permit,
            )
            .await;
            if let Err(e) = result {
                tracing::warn!(session_id = %session_id, error = %e, "sandbox engine exited with error");
            }
            registry.remove_if(&session_id, |_, h| h.token == token);
        });
    }

    pub async fn control(
        &self,
        session_id: &str,
        action: crate::models::sandbox::ControlAction,
    ) -> Result<SessionSnapshot, AppError> {
        use crate::models::sandbox::ControlAction;
        let row = load_row(&self.pool, session_id).await?;
        let status = SessionStatus::from_db(&row.status)
            .ok_or_else(|| AppError::internal("非法会话状态"))?;
        match action {
            ControlAction::Pause => {
                if status != SessionStatus::Running {
                    return Err(AppError::new(
                        ErrorCode::SessionConflict,
                        "当前状态不可暂停",
                    ));
                }
                self.transition(
                    session_id,
                    &["running"],
                    "paused",
                    SandboxEventType::SessionPaused,
                    json!({"by":"user"}),
                )
                .await?;
            }
            ControlAction::Abort => {
                if !status.is_active() {
                    return Err(AppError::new(ErrorCode::SessionConflict, "会话已终结"));
                }
                self.cancel(session_id);
                self.transition(
                    session_id,
                    &[
                        "created",
                        "running",
                        "paused",
                        "waiting_input",
                        "finalizing",
                        "interrupted",
                    ],
                    "aborted",
                    SandboxEventType::SessionAborted,
                    json!({"by":"user"}),
                )
                .await?;
            }
            _ => {
                return Err(AppError::new(
                    ErrorCode::SessionConflict,
                    "该动作需由专用端点处理",
                ));
            }
        }
        self.snapshot(session_id).await
    }

    pub async fn transition(
        &self,
        session_id: &str,
        from: &[&str],
        to: &str,
        event: SandboxEventType,
        payload: Value,
    ) -> Result<(), AppError> {
        let placeholders = vec!["?"; from.len()].join(",");
        let sql = format!(
            "UPDATE sandbox_sessions SET status = ?, updated_at = strftime('%s','now') WHERE id = ? AND status IN ({placeholders})"
        );
        let mut tx = db::begin_immediate(&self.pool).await.map_err(db_err)?;
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
            return Err(AppError::new(
                ErrorCode::SessionConflict,
                "状态已变化，操作被拒绝",
            ));
        }
        persist_event(&mut tx, session_id, event, payload).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    pub async fn input(&self, session_id: &str, text: &str) -> Result<SessionSnapshot, AppError> {
        if text.chars().count() > 2000 {
            return Err(invalid(vec![(
                "text",
                "输入不能超过 2000 字符".to_string(),
            )]));
        }
        let row = load_row(&self.pool, session_id).await?;
        let status = SessionStatus::from_db(&row.status)
            .ok_or_else(|| AppError::internal("非法会话状态"))?;
        if status != SessionStatus::WaitingInput {
            return Err(AppError::new(
                ErrorCode::SessionConflict,
                "只有 waiting_input 会话可以注入输入",
            ));
        }
        let mut tx = db::begin_immediate(&self.pool).await.map_err(db_err)?;
        let affected = sqlx::query(
            "UPDATE sandbox_sessions SET status = 'running', pending_directive = ?, updated_at = strftime('%s','now') WHERE id = ? AND status = 'waiting_input'",
        )
        .bind(text)
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if affected == 0 {
            tx.rollback().await.map_err(db_err)?;
            return Err(AppError::new(
                ErrorCode::SessionConflict,
                "会话状态已变化，输入被拒绝",
            ));
        }
        persist_event(
            &mut tx,
            session_id,
            SandboxEventType::UserInputReceived,
            json!({ "step": row.current_step, "text": text }),
        )
        .await?;
        tx.commit().await.map_err(db_err)?;
        if self.registry.contains_key(session_id) {
            self.notify_input(session_id);
        } else {
            let chain = self.build_chain();
            if chain.is_empty() {
                return Err(AppError::new(
                    ErrorCode::LlmConfigMissing,
                    "未配置 LLM API Key，无法继续沙盒",
                ));
            }
            let permit = self
                .semaphore
                .clone()
                .try_acquire_owned()
                .map_err(|_| AppError::new(ErrorCode::RateLimited, "并发沙盒会话已达上限"))?;
            let handle = self.reserve_handle(session_id)?;
            self.spawn_engine(session_id.to_string(), chain, permit, false, handle);
        }
        self.snapshot(session_id).await
    }
}

pub async fn load_row(pool: &SqlitePool, session_id: &str) -> Result<SessionRow, AppError> {
    sqlx::query_as::<_, SessionRow>("SELECT * FROM sandbox_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| not_found(ErrorCode::SessionNotFound, "会话不存在"))
}

async fn usage_summary(pool: &SqlitePool, session_id: &str) -> Result<UsageSummary, AppError> {
    let totals: (i64, i64, f64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), CAST(COALESCE(SUM(cost_usd),0) AS REAL), COUNT(*) FROM llm_usage WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT purpose, COUNT(*) FROM llm_usage WHERE session_id = ? GROUP BY purpose ORDER BY COUNT(*) DESC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(UsageSummary {
        prompt_tokens: totals.0.max(0) as u64,
        completion_tokens: totals.1.max(0) as u64,
        cost_usd: totals.2,
        calls: totals.3.max(0) as u32,
        by_purpose: rows
            .into_iter()
            .map(|(purpose, calls)| PurposeCount {
                purpose,
                calls: calls.max(0) as u32,
            })
            .collect(),
    })
}
