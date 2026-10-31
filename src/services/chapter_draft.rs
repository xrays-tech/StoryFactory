// src/services/chapter_draft.rs — 按章节大纲生成正文（逐场生成 + 文风参考）
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use dashmap::DashMap;
use serde_json::json;
use sqlx::sqlite::SqlitePool;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::config::AppConfig;
use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::llm::budget::Budget;
use crate::llm::factory;
use crate::llm::failover::ProviderChain;
use crate::llm::prompts;
use crate::llm::service::LlmService;
use crate::llm::{LlmErrorKind, Message, Purpose};
use crate::models::draft::{ChapterContent, CreateDraftReq, DraftJob, DraftStatus};
use crate::services::util::{db_err, not_found};

pub const DRAFT_SEMAPHORE: usize = 2;
pub const DEFAULT_TARGET_WORDS: i64 = 2_000;
pub const MIN_TARGET_WORDS: i64 = 500;
pub const MAX_TARGET_WORDS: i64 = 8_000;
pub const MAX_STYLE_SAMPLE_CHARS: usize = 6_000;
pub const MAX_INSTRUCTION_CHARS: usize = 1_000;
pub const MAX_SCENES: usize = 12;
pub const PREVIOUS_TAIL_CHARS: usize = 300;

const PROSE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(180);
const PROSE_STEP_TIMEOUT_SECS: u64 = 240;
const PROSE_SESSION_TIMEOUT_SECS: u64 = 3_600;

const PROSE_RULES: &str = "1. 只写大纲给定的情节，不得写大纲之外的内容。\n\
2. 不得新增原文没有的关键事实（数字、年龄、亲缘、疾病、职业）。\n\
3. 不得跳到大纲后面的情节。\n\
4. 不得写章节标题，也不得出现“第X章”。\n\
5. 不写作者旁白或说明。";

const DRAFT_COLS: &str =
    "id, chapter_id, status, stage, progress, style_name, target_words, message, error, \
     created_at, updated_at";

/// 后台任务注册表：取消令牌 + 并发闸。
#[derive(Clone)]
pub struct DraftRegistry {
    handles: Arc<DashMap<String, CancellationToken>>,
    sem: Arc<Semaphore>,
    config: Arc<ArcSwap<AppConfig>>,
    pool: SqlitePool,
    chain_override: Option<ProviderChain>,
}

impl DraftRegistry {
    pub fn new(
        config: Arc<ArcSwap<AppConfig>>,
        pool: SqlitePool,
        chain_override: Option<ProviderChain>,
    ) -> Self {
        Self {
            handles: Arc::new(DashMap::new()),
            sem: Arc::new(Semaphore::new(DRAFT_SEMAPHORE)),
            config,
            pool,
            chain_override,
        }
    }

    pub fn with_chain(
        config: Arc<ArcSwap<AppConfig>>,
        pool: SqlitePool,
        max_concurrent: usize,
        chain: ProviderChain,
    ) -> Self {
        let mut registry = Self::new(config, pool, Some(chain));
        registry.sem = Arc::new(Semaphore::new(max_concurrent.max(1)));
        registry
    }

    fn register(&self, id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.handles.insert(id.to_string(), token.clone());
        token
    }

    /// 返回 false 表示当前进程没有该任务的 runner。
    pub fn cancel(&self, id: &str) -> bool {
        match self.handles.get(id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    pub fn remove(&self, id: &str) {
        self.handles.remove(id);
    }

    fn build_chain(&self) -> ProviderChain {
        match &self.chain_override {
            Some(chain) => chain.clone(),
            None => factory::build_chain_with_attempt_timeout(
                &self.config.load(),
                PROSE_ATTEMPT_TIMEOUT,
            ),
        }
    }

    /// 校验章节与大纲后创建 queued 草稿，并立即启动后台 runner。
    pub async fn create(
        &self,
        novel_id: &str,
        chapter_id: &str,
        req: CreateDraftReq,
    ) -> Result<DraftJob, AppError> {
        let outline: Option<Option<String>> = sqlx::query_scalar(
            "SELECT c.final_outline FROM chapters c \
             JOIN novels n ON n.id = c.novel_id \
             WHERE c.id = ? AND c.novel_id = ? AND c.deleted_at IS NULL AND n.deleted_at IS NULL",
        )
        .bind(chapter_id)
        .bind(novel_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;

        let Some(outline) = outline else {
            return Err(not_found(ErrorCode::ChapterNotFound, "章节不存在或已删除"));
        };
        if outline
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .is_none()
        {
            return Err(AppError::validation(
                "该章节还没有大纲，请先推演或手写大纲",
                json!([]),
            ));
        }

        let target_words = clamp_target_words(req.target_words);
        let mut message = None;
        let style_sample = normalize_style_sample(req.style_sample, &mut message);
        let style_name = normalize_optional_text(req.style_name, usize::MAX);
        let instruction = normalize_optional_text(req.instruction, MAX_INSTRUCTION_CHARS);
        let pov = normalize_optional_text(req.pov, 20);

        let id: String = sqlx::query_scalar(
            "INSERT INTO chapter_drafts( \
                novel_id, chapter_id, status, stage, progress, style_name, style_sample, \
                instruction, target_words, pov, message \
             ) VALUES(?, ?, 'queued', 'queued', 0, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(novel_id)
        .bind(chapter_id)
        .bind(&style_name)
        .bind(&style_sample)
        .bind(&instruction)
        .bind(target_words)
        .bind(&pov)
        .bind(&message)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;

        let job = load_job(&self.pool, &id)
            .await?
            .ok_or_else(|| AppError::internal("草稿创建后读取失败"))?;

        let token = self.register(&id);
        let registry = self.clone();
        let pool = self.pool.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            run_job(pool, job_id, registry, token).await;
        });

        Ok(job)
    }
}

#[derive(sqlx::FromRow)]
struct DraftRow {
    id: String,
    chapter_id: String,
    status: String,
    stage: Option<String>,
    progress: i64,
    style_name: Option<String>,
    target_words: i64,
    message: Option<String>,
    error: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl DraftRow {
    fn into_dto(self) -> DraftJob {
        DraftJob {
            id: self.id,
            chapter_id: self.chapter_id,
            status: DraftStatus::from_db_or_default(&self.status),
            stage: self.stage,
            progress: self.progress,
            style_name: self.style_name,
            target_words: self.target_words,
            message: self.message,
            error: self.error,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub async fn load_job(pool: &SqlitePool, id: &str) -> Result<Option<DraftJob>, AppError> {
    let sql = format!("SELECT {DRAFT_COLS} FROM chapter_drafts WHERE id = ?");
    let row = sqlx::query_as::<_, DraftRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(row.map(DraftRow::into_dto))
}

pub async fn get_job(pool: &SqlitePool, id: &str) -> Result<DraftJob, AppError> {
    load_job(pool, id)
        .await?
        .ok_or_else(|| not_found(ErrorCode::ImportNotFound, "正文草稿不存在"))
}

/// 终态草稿直接删除；进行中时向 runner 发送取消信号。
pub async fn cancel_job(
    pool: &SqlitePool,
    id: &str,
    registry: &DraftRegistry,
) -> Result<(), AppError> {
    let Some(job) = load_job(pool, id).await? else {
        return Ok(());
    };
    if job.status.is_terminal() {
        sqlx::query(
            "DELETE FROM chapter_drafts WHERE id = ? \
             AND status IN ('ready','failed','cancelled')",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
        return Ok(());
    }
    if registry.cancel(id) {
        return Ok(());
    }
    // 没有活跃 runner（例如崩溃恢复窗口）：直接落终态，避免任务永久挂起。
    sqlx::query(
        "UPDATE chapter_drafts SET status = 'cancelled', stage = 'cancelled', \
         message = '用户已取消', updated_at = strftime('%s','now') \
         WHERE id = ? AND status IN ('queued','generating')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// 用户手动保存正文（覆盖当前内容）。
pub async fn load_content(
    pool: &SqlitePool,
    novel_id: &str,
    chapter_id: &str,
) -> Result<ChapterContent, AppError> {
    let row: Option<(Option<String>, i64)> = sqlx::query_as(
        "SELECT content, COALESCE(word_count, 0) FROM chapters WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(chapter_id)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (content, word_count) =
        row.ok_or_else(|| AppError::new(ErrorCode::ChapterNotFound, "章节不存在或不属于该项目"))?;
    Ok(ChapterContent {
        content: content.unwrap_or_default(),
        word_count,
    })
}

pub async fn save_content(
    pool: &SqlitePool,
    novel_id: &str,
    chapter_id: &str,
    content: &str,
) -> Result<(), AppError> {
    if content.trim().is_empty() {
        return Err(AppError::validation(
            "正文不能为空",
            json!([{"field": "content", "message": "正文不能为空"}]),
        ));
    }
    let word_count = count_words(content);
    let affected = sqlx::query(
        "UPDATE chapters SET content = ?, word_count = ?, updated_at = strftime('%s','now') \
         WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(content)
    .bind(word_count)
    .bind(chapter_id)
    .bind(novel_id)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    if affected == 0 {
        return Err(not_found(ErrorCode::ChapterNotFound, "章节不存在或已删除"));
    }
    Ok(())
}

/// 启动恢复：上次进程中断时仍在排队/生成的草稿一律置为失败。
pub async fn recover_startup(pool: &SqlitePool) -> Result<u64, AppError> {
    let affected = sqlx::query(
        "UPDATE chapter_drafts SET status = 'failed', stage = 'failed', \
         error = '服务重启中断', updated_at = strftime('%s','now') \
         WHERE status IN ('queued','generating')",
    )
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    Ok(affected)
}

// ============================================================
// 后台 runner
// ============================================================

#[derive(Debug)]
enum RunError {
    Cancelled,
    Failed(String),
}

#[derive(Debug)]
enum SceneError {
    Cancelled,
    Truncated,
    Failed(String),
}

fn db_failure(err: sqlx::Error) -> RunError {
    RunError::Failed(db_err(err).message)
}

fn ensure_active(token: &CancellationToken) -> Result<(), RunError> {
    if token.is_cancelled() {
        Err(RunError::Cancelled)
    } else {
        Ok(())
    }
}

fn ensure_scene_active(token: &CancellationToken) -> Result<(), SceneError> {
    if token.is_cancelled() {
        Err(SceneError::Cancelled)
    } else {
        Ok(())
    }
}

async fn run_job(pool: SqlitePool, id: String, registry: DraftRegistry, token: CancellationToken) {
    let outcome = run_job_inner(&pool, &id, &registry, &token).await;
    registry.remove(&id);
    let result = match outcome {
        Ok(()) => return,
        Err(RunError::Cancelled) => mark_cancelled(&pool, &id).await,
        Err(RunError::Failed(message)) => {
            if token.is_cancelled() {
                mark_cancelled(&pool, &id).await
            } else {
                mark_failed(&pool, &id, &message).await
            }
        }
    };
    if let Err(e) = result {
        tracing::warn!(draft_id = %id, error = %e, "chapter draft terminal status write failed");
    }
}

async fn mark_cancelled(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE chapter_drafts SET status = 'cancelled', stage = 'cancelled', \
         message = '用户已取消', updated_at = strftime('%s','now') \
         WHERE id = ? AND status IN ('queued','generating')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn mark_failed(pool: &SqlitePool, id: &str, message: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE chapter_drafts SET status = 'failed', stage = 'failed', error = ?, \
         updated_at = strftime('%s','now') \
         WHERE id = ? AND status IN ('queued','generating')",
    )
    .bind(message)
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

#[derive(sqlx::FromRow)]
struct DraftParams {
    novel_id: String,
    chapter_id: String,
    style_name: Option<String>,
    style_sample: Option<String>,
    instruction: Option<String>,
    target_words: i64,
    pov: Option<String>,
    message: Option<String>,
}

async fn load_params(pool: &SqlitePool, id: &str) -> Result<DraftParams, RunError> {
    sqlx::query_as::<_, DraftParams>(
        "SELECT novel_id, chapter_id, style_name, style_sample, instruction, target_words, pov, message \
         FROM chapter_drafts WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_failure)?
    .ok_or_else(|| RunError::Failed("正文草稿不存在".to_string()))
}

#[derive(sqlx::FromRow)]
struct ChapterRow {
    title: String,
    final_outline: Option<String>,
    volume_title: String,
    volume_summary: Option<String>,
}

#[derive(sqlx::FromRow)]
struct NovelRow {
    title: String,
    world_view: Option<String>,
    background: Option<String>,
    summary: Option<String>,
}

#[derive(sqlx::FromRow)]
struct CharacterRow {
    name: String,
    personality: Option<String>,
    speaking_style: Option<String>,
    current_goal: Option<String>,
}

struct ProseContext {
    novel_title: String,
    chapter_title: String,
    world_view: String,
    background: String,
    chapter_outline: String,
    characters: String,
    instruction: String,
    pov: String,
}

async fn run_job_inner(
    pool: &SqlitePool,
    id: &str,
    registry: &DraftRegistry,
    token: &CancellationToken,
) -> Result<(), RunError> {
    let permit = tokio::select! {
        permit = registry.sem.acquire() => {
            permit.map_err(|_| RunError::Failed("正文生成并发闸已关闭".to_string()))?
        }
        _ = token.cancelled() => return Err(RunError::Cancelled),
    };
    let _permit = permit;

    let affected = sqlx::query(
        "UPDATE chapter_drafts SET status = 'generating', stage = 'preparing', progress = 5, \
         error = NULL, updated_at = strftime('%s','now') \
         WHERE id = ? AND status = 'queued'",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_failure)?
    .rows_affected();
    if affected == 0 {
        // 已被其它路径取消/接管：不触碰任务状态。
        return Ok(());
    }

    ensure_active(token)?;
    let params = load_params(pool, id).await?;

    let chain = registry.build_chain();
    if chain.is_empty() {
        return Err(RunError::Failed("未配置 LLM API Key".to_string()));
    }
    let llm = LlmService::new(chain, pool.clone(), None);

    let (context, outline) = load_context(pool, &params).await?;
    let mut scenes = split_scenes(&outline);
    if scenes.is_empty() {
        scenes.push(outline.clone());
    }
    let targets = distribute_targets(params.target_words, scenes.len());
    let total = scenes.len();
    let system = prompts::render(
        prompts::CHAPTER_PROSE_SYSTEM,
        &json!({
            "style_name": params.style_name.as_deref().unwrap_or(""),
            "style_sample": params.style_sample.as_deref().unwrap_or(""),
            "rules": PROSE_RULES,
        }),
    )
    .map_err(|e| RunError::Failed(e.message))?;

    let mut content = String::new();
    let mut previous_tail = String::new();
    let mut skipped = 0usize;

    for (index, scene) in scenes.iter().enumerate() {
        ensure_active(token)?;
        let target = targets.get(index).copied().unwrap_or(params.target_words);
        match generate_scene(
            &llm,
            &system,
            &context,
            index,
            total,
            scene,
            target,
            &previous_tail,
            token,
        )
        .await
        {
            Ok(text) => {
                if !content.is_empty() {
                    content.push_str("\n\n");
                }
                content.push_str(&text);
                previous_tail = tail_chars(&content, PREVIOUS_TAIL_CHARS);
            }
            Err(SceneError::Cancelled) => return Err(RunError::Cancelled),
            Err(SceneError::Truncated) => {
                skipped += 1;
                tracing::warn!(draft_id = %id, scene = index + 1, "正文场景输出被截断，已跳过");
            }
            Err(SceneError::Failed(message)) => {
                skipped += 1;
                tracing::warn!(draft_id = %id, scene = index + 1, error = %message, "正文场景生成失败，已跳过");
            }
        }

        let progress = 5 + (90 * (index as i64 + 1)) / total as i64;
        sqlx::query(
            "UPDATE chapter_drafts SET stage = 'writing', progress = ?, \
             updated_at = strftime('%s','now') WHERE id = ? AND status = 'generating'",
        )
        .bind(progress)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_failure)?;
    }

    ensure_active(token)?;
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err(RunError::Failed("所有场景生成失败".to_string()));
    }
    let word_count = count_words(&content);
    let mut message = params.message.clone();
    if skipped > 0 {
        append_note(
            &mut message,
            format!("已跳过 {skipped} 场（输出被截断/失败）"),
        );
    }

    let mut tx = db::begin_immediate(pool).await.map_err(db_failure)?;
    let affected = sqlx::query(
        "UPDATE chapters SET content = ?, word_count = ?, updated_at = strftime('%s','now') \
         WHERE id = ? AND novel_id = ? AND deleted_at IS NULL",
    )
    .bind(&content)
    .bind(word_count)
    .bind(&params.chapter_id)
    .bind(&params.novel_id)
    .execute(&mut *tx)
    .await
    .map_err(db_failure)?
    .rows_affected();
    if affected == 0 {
        tx.rollback().await.ok();
        return Err(RunError::Failed("章节已被删除，正文未写入".to_string()));
    }
    sqlx::query(
        "UPDATE chapter_drafts SET status = 'ready', stage = 'finished', progress = 100, \
         content = ?, message = ?, error = NULL, updated_at = strftime('%s','now') \
         WHERE id = ? AND status = 'generating'",
    )
    .bind(&content)
    .bind(&message)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_failure)?;
    tx.commit().await.map_err(db_failure)?;
    Ok(())
}

async fn load_context(
    pool: &SqlitePool,
    params: &DraftParams,
) -> Result<(ProseContext, String), RunError> {
    let chapter = sqlx::query_as::<_, ChapterRow>(
        "SELECT c.title, c.final_outline, v.title AS volume_title, v.summary AS volume_summary \
         FROM chapters c JOIN volumes v ON v.id = c.volume_id \
         WHERE c.id = ? AND c.novel_id = ? AND c.deleted_at IS NULL",
    )
    .bind(&params.chapter_id)
    .bind(&params.novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_failure)?
    .ok_or_else(|| RunError::Failed("章节已被删除".to_string()))?;

    let novel = sqlx::query_as::<_, NovelRow>(
        "SELECT title, world_view, background, summary FROM novels \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&params.novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_failure)?
    .ok_or_else(|| RunError::Failed("小说不存在或已删除".to_string()))?;

    let outline = chapter.final_outline.unwrap_or_default();
    if outline.trim().is_empty() {
        return Err(RunError::Failed(
            "该章节还没有大纲，请先推演或手写大纲".to_string(),
        ));
    }

    let character_rows = sqlx::query_as::<_, CharacterRow>(
        "SELECT name, personality, speaking_style, current_goal FROM characters \
         WHERE novel_id = ? AND deleted_at IS NULL \
         ORDER BY CASE role_type \
             WHEN 'protagonist' THEN 0 WHEN 'deuteragonist' THEN 1 \
             WHEN 'antagonist' THEN 2 ELSE 3 END, created_at",
    )
    .bind(&params.novel_id)
    .fetch_all(pool)
    .await
    .map_err(db_failure)?;
    let characters: Vec<CharacterRow> = character_rows
        .into_iter()
        .filter(|character| {
            !character.name.trim().is_empty() && outline.contains(character.name.trim())
        })
        .take(8)
        .collect();

    let background = [novel.background.as_deref(), novel.summary.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let chapter_title = match chapter.volume_title.trim() {
        "" => chapter.title,
        volume_title => format!("{volume_title}·{}", chapter.title),
    };
    let chapter_outline = match chapter
        .volume_summary
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        Some(summary) => format!("（本卷梗概：{summary}）\n{outline}"),
        None => outline.clone(),
    };

    Ok((
        ProseContext {
            novel_title: novel.title,
            chapter_title,
            world_view: novel.world_view.unwrap_or_default(),
            background,
            chapter_outline,
            characters: format_characters(&characters),
            instruction: params.instruction.clone().unwrap_or_default(),
            pov: params
                .pov
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("第三人称")
                .to_string(),
        },
        outline,
    ))
}

fn format_characters(characters: &[CharacterRow]) -> String {
    if characters.is_empty() {
        return "（无）".to_string();
    }
    characters
        .iter()
        .map(|character| {
            let mut traits = Vec::new();
            if let Some(value) = character
                .personality
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                traits.push(format!("性格：{value}"));
            }
            if let Some(value) = character
                .speaking_style
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                traits.push(format!("说话风格：{value}"));
            }
            if let Some(value) = character
                .current_goal
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                traits.push(format!("当前目标：{value}"));
            }
            if traits.is_empty() {
                format!("- {}", character.name.trim())
            } else {
                format!("- {}（{}）", character.name.trim(), traits.join("；"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
async fn generate_scene(
    llm: &LlmService,
    system: &str,
    context: &ProseContext,
    scene_index: usize,
    scene_total: usize,
    scene: &str,
    target_words: i64,
    previous_tail: &str,
    token: &CancellationToken,
) -> Result<String, SceneError> {
    ensure_scene_active(token)?;
    let user = prompts::render(
        prompts::CHAPTER_PROSE_SCENE,
        &json!({
            "novel_title": &context.novel_title,
            "chapter_title": &context.chapter_title,
            "world_view": &context.world_view,
            "background": &context.background,
            "chapter_outline": &context.chapter_outline,
            "scene_index": scene_index + 1,
            "scene_total": scene_total,
            "scene_beats": scene,
            "characters": &context.characters,
            "previous_tail": previous_tail,
            "target_words": target_words,
            "instruction": &context.instruction,
            "pov": &context.pov,
        }),
    )
    .map_err(|e| SceneError::Failed(e.message))?;
    let messages = vec![Message::system(system.to_string()), Message::user(user)];
    let budget = Budget::new(
        PROSE_SESSION_TIMEOUT_SECS,
        PROSE_STEP_TIMEOUT_SECS,
        PROSE_ATTEMPT_TIMEOUT.as_secs(),
    );
    let max_tokens = scene_max_tokens(target_words);
    let result = tokio::select! {
        _ = token.cancelled() => return Err(SceneError::Cancelled),
        result = llm.complete_text(&budget, Purpose::Prose, messages.clone(), max_tokens) => result,
    };

    match result {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                Err(SceneError::Failed("模型返回空正文".to_string()))
            } else {
                Ok(text)
            }
        }
        Err(error) if error.kind == LlmErrorKind::Truncated => {
            // 推理模型的 reasoning tokens 与正文共享预算：短场景没有二分空间，先升预算重试一次。
            let escalated = max_tokens.saturating_mul(2).min(8_000);
            if escalated > max_tokens {
                let retry = tokio::select! {
                    _ = token.cancelled() => return Err(SceneError::Cancelled),
                    retry = llm.complete_text(&budget, Purpose::Prose, messages.clone(), escalated) => retry,
                };
                match retry {
                    Ok(text) => {
                        let text = text.trim().to_string();
                        if !text.is_empty() {
                            return Ok(text);
                        }
                    }
                    Err(retry_error) if retry_error.kind != LlmErrorKind::Truncated => {
                        return Err(SceneError::Failed(retry_error.message));
                    }
                    Err(_) => {}
                }
            }
            let Some((left, right)) = split_scene_text(scene) else {
                return Err(SceneError::Truncated);
            };
            let left_target = (target_words / 2).max(1);
            let right_target = (target_words - left_target).max(1);
            let left_text = Box::pin(generate_scene(
                llm,
                system,
                context,
                scene_index,
                scene_total,
                &left,
                left_target,
                previous_tail,
                token,
            ))
            .await?;
            let right_tail = tail_chars(&left_text, PREVIOUS_TAIL_CHARS);
            let right_text = Box::pin(generate_scene(
                llm,
                system,
                context,
                scene_index,
                scene_total,
                &right,
                right_target,
                &right_tail,
                token,
            ))
            .await?;
            Ok(format!("{left_text}\n\n{right_text}"))
        }
        Err(error) => Err(SceneError::Failed(error.message)),
    }
}

// ============================================================
// 纯函数（便于单测）
// ============================================================

pub fn clamp_target_words(requested: Option<i64>) -> i64 {
    requested
        .unwrap_or(DEFAULT_TARGET_WORDS)
        .clamp(MIN_TARGET_WORDS, MAX_TARGET_WORDS)
}

fn normalize_optional_text(value: Option<String>, max_chars: usize) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(max_chars).collect())
}

fn normalize_style_sample(raw: Option<String>, message: &mut Option<String>) -> Option<String> {
    let raw = raw?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let char_count = trimmed.chars().count();
    if char_count > MAX_STYLE_SAMPLE_CHARS {
        append_note(
            message,
            format!("文风参考超过 {MAX_STYLE_SAMPLE_CHARS} 字，已截断"),
        );
        Some(trimmed.chars().take(MAX_STYLE_SAMPLE_CHARS).collect())
    } else {
        Some(trimmed.to_string())
    }
}

fn append_note(note: &mut Option<String>, extra: String) {
    match note {
        Some(existing) if !existing.trim().is_empty() => {
            existing.push('；');
            existing.push_str(&extra);
        }
        _ => *note = Some(extra),
    }
}

pub fn count_words(text: &str) -> i64 {
    text.chars().filter(|c| !c.is_whitespace()).count() as i64
}

pub fn scene_max_tokens(target_words: i64) -> u32 {
    (target_words.saturating_mul(2) + 1_200).clamp(1_500, 8_000) as u32
}

pub fn distribute_targets(total_words: i64, scene_count: usize) -> Vec<i64> {
    if scene_count == 0 {
        return Vec::new();
    }
    let base = total_words / scene_count as i64;
    let remainder = total_words % scene_count as i64;
    (0..scene_count)
        .map(|index| base + if (index as i64) < remainder { 1 } else { 0 })
        .collect()
}

/// 从章节大纲抽取情节清单：优先 `## 关键场景` 列表，否则按 `## 剧情梗概` 切句。
pub fn split_scenes(outline: &str) -> Vec<String> {
    let mut scenes = extract_section(outline, "关键场景")
        .map(parse_list_items)
        .unwrap_or_default();
    if scenes.is_empty() {
        scenes = extract_section(outline, "剧情梗概")
            .map(split_summary_sentences)
            .unwrap_or_default();
    }
    scenes
        .into_iter()
        .map(|scene| scene.trim().to_string())
        .filter(|scene| !scene.is_empty())
        .take(MAX_SCENES)
        .collect()
}

fn extract_section<'a>(outline: &'a str, title: &str) -> Option<&'a str> {
    let mut start: Option<usize> = None;
    let mut cursor = 0usize;
    for line in outline.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            if let Some(section_start) = start {
                return Some(outline[section_start..cursor].trim());
            }
            let heading = trimmed.trim_start_matches('#').trim();
            if heading.starts_with(title) {
                start = Some(cursor + line.len());
            }
        }
        cursor += line.len();
    }
    start.map(|section_start| outline[section_start..].trim())
}

fn parse_list_items(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(strip_list_marker)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn strip_list_marker(line: &str) -> Option<&str> {
    let line = line.trim();
    for prefix in ["- ", "* ", "• "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = line[digits..].trim_start();
    rest.strip_prefix(". ")
        .or_else(|| rest.strip_prefix("、"))
        .or_else(|| rest.strip_prefix(") "))
        .or_else(|| rest.strip_prefix(")"))
}

fn split_summary_sentences(body: &str) -> Vec<String> {
    body.split(['。', '；', ';', '\n'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub fn split_scene_text(text: &str) -> Option<(String, String)> {
    let char_count = text.chars().count();
    if char_count < 2 {
        return None;
    }
    let split_at = text
        .char_indices()
        .nth(char_count / 2)
        .map(|(index, _)| index)?;
    let (left, right) = text.split_at(split_at);
    let (left, right) = (left.trim(), right.trim());
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left.to_string(), right.to_string()))
}

pub fn tail_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.trim().to_string();
    }
    text.chars()
        .skip(count - max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_scenes_prefers_key_scene_list() {
        let outline = "\
## 剧情梗概
少年入城。老者赠匣。

## 关键场景
- 少年推门而入
- 老者递出木匣
- 木匣中剑光乍现

## 人物变化
少年变得谨慎。";
        assert_eq!(
            split_scenes(outline),
            vec!["少年推门而入", "老者递出木匣", "木匣中剑光乍现"]
        );
    }

    #[test]
    fn split_scenes_falls_back_to_summary_sentences() {
        let outline = "## 剧情梗概\n少年推门而入。老者递出木匣；木匣中剑光乍现\n";
        assert_eq!(
            split_scenes(outline),
            vec!["少年推门而入", "老者递出木匣", "木匣中剑光乍现"]
        );
    }

    #[test]
    fn split_scenes_returns_empty_without_outline() {
        assert!(split_scenes("").is_empty());
        assert!(split_scenes("## 人物变化\n无").is_empty());
    }

    #[test]
    fn target_words_are_clamped() {
        assert_eq!(clamp_target_words(None), DEFAULT_TARGET_WORDS);
        assert_eq!(clamp_target_words(Some(10)), MIN_TARGET_WORDS);
        assert_eq!(clamp_target_words(Some(99_999)), MAX_TARGET_WORDS);
        assert_eq!(clamp_target_words(Some(1_234)), 1_234);
    }

    #[test]
    fn targets_are_distributed_without_losing_remainder() {
        assert_eq!(distribute_targets(10, 3), vec![4, 3, 3]);
        assert_eq!(distribute_targets(9, 3), vec![3, 3, 3]);
        assert!(distribute_targets(10, 0).is_empty());
    }

    #[test]
    fn truncated_scene_can_be_split_at_char_boundary() {
        let (left, right) = split_scene_text("甲乙丙丁").expect("splittable");
        assert_eq!(left, "甲乙");
        assert_eq!(right, "丙丁");
        assert!(split_scene_text("甲").is_none());
    }

    #[test]
    fn tail_keeps_last_characters() {
        assert_eq!(tail_chars("一二三四五", 3), "三四五");
        assert_eq!(tail_chars("短", 3), "短");
    }
}
