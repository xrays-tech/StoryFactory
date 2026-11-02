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
use crate::models::draft::{
    ChapterContent, CreateDraftReq, DraftJob, DraftLint, DraftLintIssue, DraftStage, DraftStatus,
};
use crate::services::prose_lint::{self, LintKind};
use crate::services::util::{db_err, not_found};

pub const DRAFT_SEMAPHORE: usize = 2;
pub const DEFAULT_TARGET_WORDS: i64 = 2_000;
/// 章节未配置目标字数时的兜底值（与迁移 0010 的列默认一致）。
pub const DEFAULT_CHAPTER_TARGET_WORDS: i64 = 3_500;
pub const MIN_TARGET_WORDS: i64 = 500;
pub const MAX_TARGET_WORDS: i64 = 8_000;
pub const MAX_STYLE_SAMPLE_CHARS: usize = 6_000;
pub const MAX_INSTRUCTION_CHARS: usize = 1_000;
pub const MAX_SCENES: usize = 12;
pub const PREVIOUS_TAIL_CHARS: usize = 120;
const COMPLETED_SCENE_TITLE_CHARS: usize = 40;

const PROSE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(300);
const PROSE_STEP_TIMEOUT_SECS: u64 = 480;
const PROSE_SESSION_TIMEOUT_SECS: u64 = 5_400;

const DESLOP_SYSTEM: &str = "你是小说编辑。只改写用户给出的这一段正文，只输出改写后的纯正文。";

const PROSE_RULES: &str = "1. 只写大纲给定的情节，不得写大纲之外的内容。\n\
2. 不得新增原文没有的关键事实（数字、年龄、亲缘、疾病、职业）。\n\
3. 不得跳到大纲后面的情节。\n\
4. 不得写章节标题，也不得出现“第X章”。\n\
5. 不写作者旁白或说明。";

const DRAFT_COLS: &str =
    "id, chapter_id, status, stage, progress, style_name, target_words, message, error, \
     lint_json, created_at, updated_at";

/// 章节大纲 + 小说文风档案（一次查询完成回退所需的读取）。
#[derive(sqlx::FromRow)]
struct ChapterStyleRow {
    final_outline: Option<String>,
    style_name: Option<String>,
    style_sample: Option<String>,
    style_instruction: Option<String>,
    pov: Option<String>,
    target_words: i64,
}

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
        let chapter = sqlx::query_as::<_, ChapterStyleRow>(
            "SELECT c.final_outline, n.style_name, n.style_sample, n.style_instruction, n.pov, \
             c.target_words \
             FROM chapters c \
             JOIN novels n ON n.id = c.novel_id \
             WHERE c.id = ? AND c.novel_id = ? AND c.deleted_at IS NULL AND n.deleted_at IS NULL",
        )
        .bind(chapter_id)
        .bind(novel_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;

        let Some(chapter) = chapter else {
            return Err(not_found(ErrorCode::ChapterNotFound, "章节不存在或已删除"));
        };
        if chapter
            .final_outline
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

        // 文风优先级：请求 > 小说档案 > 默认（叙述人称默认第三人称，在 ProseContext 处理）。
        // 目标字数优先级：请求 > 章节设定 > 默认 3500；非正值视为未提供。
        let target_words = resolve_target_words(req.target_words, Some(chapter.target_words));
        let mut message = None;
        let style_sample = normalize_style_sample(
            first_non_blank(req.style_sample, chapter.style_sample),
            &mut message,
        );
        let style_name = normalize_optional_text(
            first_non_blank(req.style_name, chapter.style_name),
            usize::MAX,
        );
        let instruction = normalize_optional_text(
            first_non_blank(req.instruction, chapter.style_instruction),
            MAX_INSTRUCTION_CHARS,
        );
        let pov = normalize_optional_text(first_non_blank(req.pov, chapter.pov), 20);
        let deslop = req
            .deslop
            .map(|enabled| if enabled { 1_i64 } else { 0_i64 });

        let id: String = sqlx::query_scalar(
            "INSERT INTO chapter_drafts( \
                novel_id, chapter_id, status, stage, progress, style_name, style_sample, \
                instruction, target_words, pov, deslop, message \
             ) VALUES(?, ?, 'queued', ?, 0, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(novel_id)
        .bind(chapter_id)
        .bind(DraftStage::Queued.as_str())
        .bind(&style_name)
        .bind(&style_sample)
        .bind(&instruction)
        .bind(target_words)
        .bind(&pov)
        .bind(deslop)
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
    lint_json: Option<String>,
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
            lint: self
                .lint_json
                .and_then(|json| serde_json::from_str::<DraftLint>(&json).ok()),
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
        "UPDATE chapter_drafts SET status = 'cancelled', stage = ?, \
         message = '用户已取消', updated_at = strftime('%s','now') \
         WHERE id = ? AND status IN ('queued','generating')",
    )
    .bind(DraftStage::Cancelled.as_str())
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
    let lint_json: Option<Option<String>> = sqlx::query_scalar(
        "SELECT lint_json FROM chapter_drafts \
         WHERE chapter_id = ? AND novel_id = ? \
         ORDER BY created_at DESC, updated_at DESC LIMIT 1",
    )
    .bind(chapter_id)
    .bind(novel_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let lint = lint_json.flatten().and_then(|json| {
        serde_json::from_str::<DraftLint>(&json)
            .map_err(|error| {
                tracing::warn!(chapter_id, error = %error, "草稿 lint_json 解析失败，忽略");
            })
            .ok()
    });
    Ok(ChapterContent {
        content: content.unwrap_or_default(),
        word_count,
        lint,
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
        "UPDATE chapter_drafts SET status = 'failed', stage = ?, \
         error = '服务重启中断', updated_at = strftime('%s','now') \
         WHERE status IN ('queued','generating')",
    )
    .bind(DraftStage::Failed.as_str())
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

/// 单场生成结果：`missing_part` 表示二分后有一半彻底失败，已保留另一半。
#[derive(Debug)]
struct SceneOutcome {
    text: String,
    missing_part: bool,
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
        "UPDATE chapter_drafts SET status = 'cancelled', stage = ?, \
         message = '用户已取消', updated_at = strftime('%s','now') \
         WHERE id = ? AND status IN ('queued','generating')",
    )
    .bind(DraftStage::Cancelled.as_str())
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn mark_failed(pool: &SqlitePool, id: &str, message: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE chapter_drafts SET status = 'failed', stage = ?, error = ?, \
         updated_at = strftime('%s','now') \
         WHERE id = ? AND status IN ('queued','generating')",
    )
    .bind(DraftStage::Failed.as_str())
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
    deslop: Option<i64>,
    message: Option<String>,
}

async fn load_params(pool: &SqlitePool, id: &str) -> Result<DraftParams, RunError> {
    sqlx::query_as::<_, DraftParams>(
        "SELECT novel_id, chapter_id, style_name, style_sample, instruction, target_words, \
         pov, deslop, message \
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
}

#[derive(sqlx::FromRow)]
struct NovelRow {
    title: String,
    world_view: Option<String>,
    background: Option<String>,
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
        "UPDATE chapter_drafts SET status = 'generating', stage = ?, progress = 5, \
         error = NULL, updated_at = strftime('%s','now') \
         WHERE id = ? AND status = 'queued'",
    )
    .bind(DraftStage::Preparing.as_str())
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
    let mut missing_scenes: Vec<usize> = Vec::new();

    for (index, scene) in scenes.iter().enumerate() {
        ensure_active(token)?;
        let target = targets.get(index).copied().unwrap_or(params.target_words);
        match generate_scene(
            &llm,
            &system,
            &context,
            &scenes,
            index,
            total,
            scene,
            target,
            &previous_tail,
            token,
        )
        .await
        {
            Ok(outcome) => {
                // 每场都会拿到上一场结尾作为衔接上文，模型常把这段又复述一遍。
                let text = if content.is_empty() {
                    outcome.text
                } else {
                    dedupe_scene_junction(&outcome.text, &content)
                };
                if text.trim().is_empty() {
                    tracing::warn!(draft_id = %id, scene = index + 1, "正文场景与上一场完全重复，已跳过");
                    continue;
                }
                if outcome.missing_part {
                    // 二分后有一半失败：正文只缺一小段，不能整场丢弃。
                    missing_scenes.push(index + 1);
                    tracing::warn!(
                        draft_id = %id,
                        scene = index + 1,
                        "正文场景部分内容缺失，已保留已生成部分"
                    );
                }
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
            "UPDATE chapter_drafts SET stage = ?, progress = ?, \
             updated_at = strftime('%s','now') WHERE id = ? AND status = 'generating'",
        )
        .bind(DraftStage::Writing.as_str())
        .bind(progress)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_failure)?;
    }

    ensure_active(token)?;
    let content = collapse_adjacent_duplicates(content.trim());
    if content.is_empty() {
        return Err(RunError::Failed("所有场景生成失败".to_string()));
    }

    let (content, lint) = run_deslop(
        pool,
        id,
        &llm,
        &content,
        params.deslop.unwrap_or(1) != 0,
        params.style_name.as_deref().unwrap_or(""),
        &context.pov,
        token,
    )
    .await?;
    ensure_active(token)?;
    let word_count = count_words(&content);
    let lint_json = serde_json::to_string(&lint).ok();

    let mut message = params.message.clone();
    if skipped > 0 {
        append_note(
            &mut message,
            format!("已跳过 {skipped} 场（输出被截断/失败）"),
        );
    }
    if !missing_scenes.is_empty() {
        let scenes = missing_scenes
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join("、");
        append_note(&mut message, format!("第 {scenes} 场部分内容缺失"));
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
        "UPDATE chapter_drafts SET status = 'ready', stage = ?, progress = 100, \
         content = ?, message = ?, error = NULL, lint_json = ?, \
         updated_at = strftime('%s','now') \
         WHERE id = ? AND status = 'generating'",
    )
    .bind(DraftStage::Finished.as_str())
    .bind(&content)
    .bind(&message)
    .bind(&lint_json)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_failure)?;
    tx.commit().await.map_err(db_failure)?;
    Ok(())
}

/// 生成后 lint；命中阈值时定点重写最多 3 段。返回最终正文与报告。
#[allow(clippy::too_many_arguments)]
async fn run_deslop(
    pool: &SqlitePool,
    id: &str,
    llm: &LlmService,
    content: &str,
    enabled: bool,
    style_name: &str,
    pov: &str,
    token: &CancellationToken,
) -> Result<(String, DraftLint), RunError> {
    ensure_active(token)?;
    update_stage(pool, id, DraftStage::Linting, 90).await?;
    let before = prose_lint::lint(content);
    let before_total = before.total;

    if !enabled || !prose_lint::needs_rewrite(&before) {
        return Ok((content.to_string(), draft_lint(&before, before_total, 0)));
    }

    update_stage(pool, id, DraftStage::Rewriting, 95).await?;
    let targets = prose_lint::rewrite_targets(content, &before, 3);
    let mut rewrites: Vec<(usize, String)> = Vec::new();
    for (index, paragraph, reasons) in targets {
        ensure_active(token)?;
        match rewrite_paragraph(llm, pov, style_name, &paragraph, &reasons, token).await? {
            Some(rewritten) => rewrites.push((index, rewritten)),
            None => {
                tracing::warn!(draft_id = %id, paragraph = index, "去 AI 味重写失败，已跳过该段");
            }
        }
    }

    if rewrites.is_empty() {
        return Ok((content.to_string(), draft_lint(&before, before_total, 0)));
    }

    let rewritten_content = apply_paragraph_rewrites(content, &rewrites);
    let after = prose_lint::lint(&rewritten_content);
    let report = draft_lint(&after, before_total, rewrites.len() as i64);
    Ok((rewritten_content, report))
}

async fn update_stage(
    pool: &SqlitePool,
    id: &str,
    stage: DraftStage,
    progress: i64,
) -> Result<(), RunError> {
    sqlx::query(
        "UPDATE chapter_drafts SET stage = ?, progress = ?, updated_at = strftime('%s','now') \
         WHERE id = ? AND status = 'generating'",
    )
    .bind(stage.as_str())
    .bind(progress)
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_failure)?;
    Ok(())
}

async fn rewrite_paragraph(
    llm: &LlmService,
    pov: &str,
    style_name: &str,
    paragraph: &str,
    reasons: &[LintKind],
    token: &CancellationToken,
) -> Result<Option<String>, RunError> {
    ensure_active(token)?;
    let reasons = reasons
        .iter()
        .map(|kind| kind.reason())
        .collect::<Vec<_>>()
        .join("；");
    let user = match prompts::render(
        prompts::PROSE_DESLOP,
        &json!({
            "paragraph": paragraph,
            "reasons": reasons,
            "pov": pov,
            "style_name": style_name,
        }),
    ) {
        Ok(user) => user,
        Err(error) => {
            tracing::warn!(error = %error.message, "去 AI 味 prompt 渲染失败，已跳过该段");
            return Ok(None);
        }
    };

    let messages = vec![
        Message::system(DESLOP_SYSTEM.to_string()),
        Message::user(user),
    ];
    let budget = Budget::new(
        PROSE_SESSION_TIMEOUT_SECS,
        PROSE_STEP_TIMEOUT_SECS,
        PROSE_ATTEMPT_TIMEOUT.as_secs(),
    );
    let max_tokens = deslop_max_tokens(paragraph);
    let result = tokio::select! {
        _ = token.cancelled() => return Err(RunError::Cancelled),
        result = llm.complete_text(&budget, Purpose::Prose, messages, max_tokens) => result,
    };

    match result {
        Ok(text) => match normalize_rewrite(&text, paragraph) {
            Some(rewritten) => Ok(Some(rewritten)),
            None => {
                tracing::warn!(
                    paragraph_len = paragraph.chars().count(),
                    "去 AI 味重写结果异常（过长或多段回显），已跳过该段"
                );
                Ok(None)
            }
        },
        Err(error) => {
            tracing::warn!(error = %error.message, "去 AI 味重写调用失败，已跳过该段");
            Ok(None)
        }
    }
}

fn draft_lint(report: &prose_lint::LintReport, before_total: i64, rewritten: i64) -> DraftLint {
    DraftLint {
        before_total,
        after_total: report.total,
        rewritten,
        issues: report
            .issues
            .iter()
            .map(|issue| DraftLintIssue {
                kind: issue.kind.as_str().to_string(),
                label: issue.kind.label().to_string(),
                count: issue.count,
            })
            .collect(),
    }
}

fn deslop_max_tokens(paragraph: &str) -> u32 {
    (count_words(paragraph).saturating_mul(2) + 400).clamp(800, 4_000) as u32
}

/// 只接受「单段」重写结果：取首个非空行，并拒绝明显超出原段长度的回显（模型有时会把后续段落一起返回）。
/// 去掉相邻的完全相同段落（模型偶发把同一句连写两遍）。
///
/// 只处理“紧挨着、内容完全一致（忽略空白）”的段落，正常重复出现的对白（相隔较远）保持不动。
pub fn collapse_adjacent_duplicates(text: &str) -> String {
    let compact = |line: &str| line.split_whitespace().collect::<String>();
    let mut out: Vec<&str> = Vec::new();
    let mut previous_key: Option<String> = None;
    for line in text.split('\n') {
        let key = compact(line);
        if !key.is_empty() && Some(&key) == previous_key.as_ref() {
            continue;
        }
        previous_key = if key.is_empty() { None } else { Some(key) };
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

/// 去掉新场景开头对上一场结尾的复述。
///
/// 模型每场都能看到「上一场结尾」作为衔接上下文，常会把这段原样再写一遍，
/// 于是正文在场景拼接处出现整块重复。这里按段落去重，只在前几段范围内生效，
/// 且如果去重后剩得太少就保留原样（宁可重复，也不吞掉真正的正文）。
pub fn dedupe_scene_junction(new_text: &str, accumulated: &str) -> String {
    const LOOKBACK_PARAGRAPHS: usize = 12;
    const JUNCTION_PARAGRAPHS: usize = 6;
    const MIN_KEEP_RATIO: f64 = 0.5;

    let compact = |text: &str| text.split_whitespace().collect::<String>();
    let previous: Vec<String> = accumulated
        .split('\n')
        .map(compact)
        .filter(|p| p.chars().count() >= 8)
        .collect::<Vec<_>>();
    if previous.is_empty() {
        return new_text.to_string();
    }
    let lookback: Vec<&String> = previous.iter().rev().take(LOOKBACK_PARAGRAPHS).collect();

    let paragraphs: Vec<&str> = new_text.split('\n').collect();
    let keys: Vec<String> = paragraphs.iter().map(|p| compact(p.trim())).collect();
    let candidates: Vec<usize> = keys
        .iter()
        .enumerate()
        .filter(|(_, key)| {
            key.chars().count() >= 8 && lookback.iter().any(|p| p.as_str() == key.as_str())
        })
        .map(|(index, _)| index)
        .collect();
    // 孤立的单段重复可能是作者有意呼应；只有成簇出现（相邻两段以内）或贴着开头，才判定为复述。
    let drop: Vec<bool> = (0..paragraphs.len())
        .map(|index| {
            candidates.contains(&index)
                && (index < JUNCTION_PARAGRAPHS
                    || candidates
                        .iter()
                        .any(|other| *other != index && other.abs_diff(index) <= 2))
        })
        .collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut dropped = 0usize;
    for (index, paragraph) in paragraphs.iter().enumerate() {
        if drop[index] {
            dropped += 1;
            continue;
        }
        kept.push(paragraph);
    }

    if dropped == 0 {
        return new_text.to_string();
    }
    let candidate = kept.join("\n");
    let candidate_chars = compact(&candidate).chars().count();
    let original_chars = compact(new_text).chars().count();
    if (candidate_chars as f64) < (original_chars as f64) * MIN_KEEP_RATIO {
        return new_text.to_string();
    }
    candidate.trim_start().to_string()
}

fn normalize_rewrite(raw: &str, original: &str) -> Option<String> {
    let candidate = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    if candidate.is_empty() {
        return None;
    }
    let original_len = original.chars().count();
    let limit = original_len.saturating_mul(2) + 80;
    if candidate.chars().count() > limit {
        return None;
    }
    // 模型偶尔把原段连同后续段落一起回显，拼进单段会制造重复正文。
    let anchor = original.trim();
    if !anchor.is_empty() && candidate.contains(anchor) && candidate.chars().count() > original_len
    {
        return None;
    }
    Some(candidate.to_string())
}

/// 按段索引替换：空行与未命中的行原样保留。
fn apply_paragraph_rewrites(text: &str, rewrites: &[(usize, String)]) -> String {
    if rewrites.is_empty() {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut paragraph_index = 0usize;
    for (line_index, line) in text.split('\n').enumerate() {
        if line_index > 0 {
            result.push('\n');
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            result.push_str(line);
            continue;
        }
        match rewrites.iter().find(|(index, _)| *index == paragraph_index) {
            Some((_, replacement)) => result.push_str(replacement),
            None => result.push_str(line),
        }
        paragraph_index += 1;
    }
    result
}

async fn load_context(
    pool: &SqlitePool,
    params: &DraftParams,
) -> Result<(ProseContext, String), RunError> {
    let chapter = sqlx::query_as::<_, ChapterRow>(
        "SELECT c.title, c.final_outline, v.title AS volume_title \
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
        "SELECT title, world_view, background FROM novels \
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

    // 只注入作品背景：全书摘要含结局，会污染本章正文（本卷梗概已在 chapter_outline 中给出）。
    let background = novel
        .background
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
        .to_string();
    let chapter_title = match chapter.volume_title.trim() {
        "" => chapter.title,
        volume_title => format!("{volume_title}·{}", chapter.title),
    };

    Ok((
        ProseContext {
            novel_title: novel.title,
            chapter_title,
            world_view: novel.world_view.unwrap_or_default(),
            background,
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

/// 已完成场次清单：只给标题，避免后续场次重看整章大纲后重演前文。
fn completed_scenes_block(scenes: &[String]) -> String {
    if scenes.is_empty() {
        return String::new();
    }
    let items = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| format!("- 第{}场：{}", index + 1, scene_title(scene)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("## 已完成场次（均已写完，禁止重演）\n{items}")
}

/// 后续场次清单：只给标题，并明确尚未发生，避免本场提前写后面的情节。
fn upcoming_scenes_block(scenes: &[String], offset: usize) -> String {
    if scenes.is_empty() {
        return String::new();
    }
    let items = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| format!("- 第{}场：{}", offset + index + 1, scene_title(scene)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("## 后续场次（尚未发生，本场禁止提前写）\n{items}")
}

/// 场景标题取首句，超长截断，保证清单本身足够短。
fn scene_title(scene: &str) -> String {
    let first_sentence = scene
        .split(['\n', '。', '；', ';'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or("");
    first_sentence
        .chars()
        .take(COMPLETED_SCENE_TITLE_CHARS)
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn generate_scene(
    llm: &LlmService,
    system: &str,
    context: &ProseContext,
    scenes: &[String],
    scene_index: usize,
    scene_total: usize,
    scene: &str,
    target_words: i64,
    previous_tail: &str,
    token: &CancellationToken,
) -> Result<SceneOutcome, SceneError> {
    ensure_scene_active(token)?;
    // 任何一场都不注入整章大纲：整章大纲会让第 1 场把后面的 beat 提前演完，
    // 后续场次只能换写法重演（用户报的“翻出栏杆又回来”）。只给已完成场次与后续场次标题。
    let chapter_outline = "";
    let completed_scenes = completed_scenes_block(&scenes[..scene_index]);
    let upcoming_scenes = upcoming_scenes_block(&scenes[scene_index + 1..], scene_index + 1);
    let user = prompts::render(
        prompts::CHAPTER_PROSE_SCENE,
        &json!({
            "novel_title": &context.novel_title,
            "chapter_title": &context.chapter_title,
            "world_view": &context.world_view,
            "background": &context.background,
            "chapter_outline": chapter_outline,
            "completed_scenes": completed_scenes,
            "upcoming_scenes": upcoming_scenes,
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
    tracing::info!(
        system_chars = messages[0].content.chars().count(),
        user_chars = messages[1].content.chars().count(),
        target_words,
        max_tokens,
        "prose scene request"
    );
    let result = tokio::select! {
        _ = token.cancelled() => return Err(SceneError::Cancelled),
        result = llm.complete_text_creative(
            &budget,
            Purpose::Prose,
            messages.clone(),
            max_tokens,
        ) => result,
    };

    match result {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                Err(SceneError::Failed("模型返回空正文".to_string()))
            } else {
                Ok(SceneOutcome {
                    text,
                    missing_part: false,
                })
            }
        }
        Err(error) if error.kind == LlmErrorKind::Truncated => {
            // service 层已按原预算重试并升一次预算；仍截断说明整场写不完，
            // 直接二分本场情节并按预算比例生成两半（宁慢勿丢）。
            let Some((left, right)) = split_scene_half(scene) else {
                return Err(SceneError::Truncated);
            };
            let left_target = (target_words / 2).max(1);
            let right_target = (target_words - left_target).max(1);
            let left_result = Box::pin(generate_scene(
                llm,
                system,
                context,
                scenes,
                scene_index,
                scene_total,
                &left,
                left_target,
                previous_tail,
                token,
            ))
            .await;
            let right_tail = match &left_result {
                Ok(outcome) => tail_chars(&outcome.text, PREVIOUS_TAIL_CHARS),
                Err(SceneError::Cancelled) => return Err(SceneError::Cancelled),
                Err(_) => previous_tail.to_string(),
            };
            let right_result = Box::pin(generate_scene(
                llm,
                system,
                context,
                scenes,
                scene_index,
                scene_total,
                &right,
                right_target,
                &right_tail,
                token,
            ))
            .await;
            combine_halves(left_result, right_result)
        }
        Err(error) => Err(SceneError::Failed(error.message)),
    }
}

/// 合并二分生成结果：任一半成功即保留，缺的一半由调用方在 message 中标注。
fn combine_halves(
    left: Result<SceneOutcome, SceneError>,
    right: Result<SceneOutcome, SceneError>,
) -> Result<SceneOutcome, SceneError> {
    match (left, right) {
        (Err(SceneError::Cancelled), _) | (_, Err(SceneError::Cancelled)) => {
            Err(SceneError::Cancelled)
        }
        (Ok(left), Ok(right)) => Ok(SceneOutcome {
            text: format!("{}\n\n{}", left.text, right.text),
            missing_part: left.missing_part || right.missing_part,
        }),
        (Ok(only), Err(_)) | (Err(_), Ok(only)) => Ok(SceneOutcome {
            text: only.text,
            missing_part: true,
        }),
        (Err(error), Err(_)) => Err(error),
    }
}

// ============================================================
// 纯函数（便于单测）
// ============================================================

pub fn clamp_target_words(requested: Option<i64>) -> i64 {
    clamp_words(requested.unwrap_or(DEFAULT_TARGET_WORDS))
}

/// 目标字数决议：请求值 > 章节目标字数 > 默认 3500；请求非正值视为未提供。
pub fn resolve_target_words(requested: Option<i64>, chapter: Option<i64>) -> i64 {
    let requested = requested.filter(|value| *value > 0);
    clamp_words(
        requested
            .or(chapter)
            .unwrap_or(DEFAULT_CHAPTER_TARGET_WORDS),
    )
}

fn clamp_words(value: i64) -> i64 {
    value.clamp(MIN_TARGET_WORDS, MAX_TARGET_WORDS)
}

fn normalize_optional_text(value: Option<String>, max_chars: usize) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(max_chars).collect())
}

/// 字段级回退：主值缺失/全空白时取备用值；两处都空白按未设置处理。
fn first_non_blank(primary: Option<String>, fallback: Option<String>) -> Option<String> {
    let clean = |value: String| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };
    primary.and_then(clean).or_else(|| fallback.and_then(clean))
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

/// 中文场景里 1 字≈1 token，推理模型还要额外消耗 reasoning token，
/// 因此按目标字数的 3 倍再加固定余量，避免每场都撞 finish_reason=length。
pub fn scene_max_tokens(target_words: i64) -> u32 {
    // 中文正文 token ≈ 字数，留一倍余量足够；预算越小，推理模型偶发“把预算全耗在推理上”时
    // 每次失败越快，重试成本越低（真正不够时仍会升级到 escalated_tokens）。
    (target_words.saturating_mul(2) + 1_500).clamp(1_500, 12_000) as u32
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

/// 把本场情节文本二分：优先在中间附近的句号/换行处切，找不到再按字符中点切。
pub fn split_scene_half(text: &str) -> Option<(String, String)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.len() < 2 {
        return None;
    }
    let mid = chars.len() / 2;
    let is_break = |c: char| matches!(c, '。' | '！' | '？' | '；' | ';' | '\n');
    // 切点在断句符之后（1..len 之间），取离中点最近的一个。
    let boundary = (1..chars.len())
        .filter(|&index| is_break(chars[index - 1].1))
        .min_by_key(|&index| index.abs_diff(mid))
        .unwrap_or(mid);
    let split_at = chars.get(boundary).map(|(index, _)| *index)?;
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
    fn collapse_adjacent_duplicates_drops_repeated_lines_only_when_adjacent() {
        let text = "手放下来了。\n手放下来了。\n\"哥。\"\n\n\"哥。\"";
        assert_eq!(
            collapse_adjacent_duplicates(text),
            "手放下来了。\n\"哥。\"\n\n\"哥。\""
        );
        assert_eq!(collapse_adjacent_duplicates("  \n\n "), "");
    }

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
    fn scene_max_tokens_leaves_room_for_reasoning() {
        assert_eq!(scene_max_tokens(0), 1_500);
        assert_eq!(scene_max_tokens(300), 2_100);
        assert_eq!(scene_max_tokens(10_000), 12_000);
    }

    #[test]
    fn prose_prompts_stay_compact() {
        let system = crate::llm::prompts::CHAPTER_PROSE_SYSTEM;
        let scene = crate::llm::prompts::CHAPTER_PROSE_SCENE;
        let system_chars = system.chars().count();
        let scene_chars = scene.chars().count();
        assert!(system_chars <= 450, "system prompt {system_chars} chars");
        assert!(scene_chars <= 700, "scene prompt {scene_chars} chars");
        // 加入场景边界硬约束后 scene 为 679 字符；system 409 + scene 679 = 1088，留 12 字符余量。
        assert!(
            system_chars + scene_chars <= 1_100,
            "prose prompts too large: {system_chars} + {scene_chars}"
        );
    }

    #[test]
    fn dedupe_scene_junction_drops_echoed_block() {
        let previous =
            "她抹了一把脸，往后退。\n“兄妹……兄妹！”她忽然喊起来。\n门被带得重撞，弹回来。";
        let next = "“兄妹……兄妹！”她忽然喊起来。\n门被带得重撞，弹回来。\n我站在原地没动，灶上的水开了。\n窗纸透进一点灰白。\n我把碗端起来，又放下。";
        let out = dedupe_scene_junction(next, previous);
        assert_eq!(
            out,
            "我站在原地没动，灶上的水开了。\n窗纸透进一点灰白。\n我把碗端起来，又放下。"
        );
    }

    #[test]
    fn dedupe_scene_junction_keeps_late_repeats() {
        let previous = "门被带得重撞，弹回来。";
        let mut next = String::from("我走进院子，风很凉。");
        for _ in 0..12 {
            next.push_str("\n檐下的灯晃了一下。");
        }
        next.push_str("\n门被带得重撞，弹回来。");
        assert_eq!(dedupe_scene_junction(&next, previous), next);
    }

    #[test]
    fn dedupe_scene_junction_keeps_text_when_most_would_be_dropped() {
        let previous = "第一段重复内容够长了吧。\n第二段重复内容够长了吧。";
        let next = "第一段重复内容够长了吧。\n第二段重复内容够长了吧。\n短。";
        assert_eq!(dedupe_scene_junction(next, previous), next);
    }

    #[test]
    fn normalize_rewrite_keeps_single_paragraph() {
        let original = "他推开门，屋里没有人。";
        assert_eq!(
            normalize_rewrite("他推门进去，屋里空着。\n\n多余的段落。", original).as_deref(),
            Some("他推门进去，屋里空着。")
        );
    }

    #[test]
    fn normalize_rewrite_rejects_echoed_tail() {
        let original = "他推开门，屋里没有人。";
        let echoed =
            format!("{original}她坐在窗边，手里捏着一封信，信封已经拆开，边角被摩挲得起毛。");
        assert!(normalize_rewrite(&echoed, original).is_none());
    }

    #[test]
    fn target_words_prefer_request_then_chapter() {
        assert_eq!(resolve_target_words(None, Some(4_200)), 4_200);
        assert_eq!(resolve_target_words(Some(0), Some(4_200)), 4_200);
        assert_eq!(resolve_target_words(Some(-3), Some(4_200)), 4_200);
        assert_eq!(resolve_target_words(Some(900), Some(4_200)), 900);
        assert_eq!(
            resolve_target_words(None, None),
            DEFAULT_CHAPTER_TARGET_WORDS
        );
        assert_eq!(resolve_target_words(Some(100), None), MIN_TARGET_WORDS);
        assert_eq!(resolve_target_words(None, Some(99_999)), MAX_TARGET_WORDS);
    }

    #[test]
    fn truncated_scene_can_be_split_at_char_boundary() {
        let (left, right) = split_scene_half("甲乙丙丁").expect("splittable");
        assert_eq!(left, "甲乙");
        assert_eq!(right, "丙丁");
        assert!(split_scene_half("甲").is_none());
    }

    #[test]
    fn truncated_scene_prefers_sentence_boundary() {
        let (left, right) = split_scene_half("少年推门。老者抬头。").expect("splittable");
        assert_eq!(left, "少年推门。");
        assert_eq!(right, "老者抬头。");
    }

    #[test]
    fn tail_keeps_last_characters() {
        assert_eq!(tail_chars("一二三四五", 3), "三四五");
        assert_eq!(tail_chars("短", 3), "短");
    }
}
