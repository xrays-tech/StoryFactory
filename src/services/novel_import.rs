// src/services/novel_import.rs — txt 小说逆向分析任务（分块 → 合并 → 结构 → 落库）
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::config::AppConfig;
use crate::db;
use crate::error::{AppError, ErrorCode};
use crate::llm::budget::Budget;
use crate::llm::factory;
use crate::llm::failover::ProviderChain;
use crate::llm::prompts;
use crate::llm::schemas;
use crate::llm::service::LlmService;
use crate::llm::{LlmError, LlmErrorKind, Message, Purpose};
use crate::models::import::{
    AnalysisData, CreateNovelFromImportReq, NovelImportJob, NovelImportPreview, NovelImportStage,
    NovelImportStatus, PreviewChapter, PreviewCharacter, PreviewItem, PreviewRelation,
    PreviewVolume,
};
use crate::services::util::{db_err, import_name, invalid, new_id, not_found};
use crate::services::{novel, search};

pub const IMPORT_SEMAPHORE: usize = 2;
pub const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;
pub const MIN_IMPORT_CHARS: usize = 200;
pub const CHUNK_CHARS: usize = 6000;
pub const CHUNK_OVERLAP: usize = 200;
pub const MAX_CHUNKS: usize = 400;
/// 分块阶段同时进行的 LLM 调用上限。
pub const IMPORT_CHUNK_CONCURRENCY: usize = 3;

const SUMMARY_ITEM_LIMIT: usize = 300;
const SUMMARY_JOIN_LIMIT: usize = 12000;
const SUMMARY_MERGE_GROUP: usize = 20;
const ANALYSIS_RETRIES: usize = 1;

/// 每章大纲中「关键场景」最多保留的条数。
const CHAPTER_BEATS_LIMIT: usize = 8;
/// 结构阶段每批最多送入的章节数。
const STRUCTURE_BATCH_SIZE: usize = 30;
/// 结构阶段单章摘要的截断长度，避免超长上下文。
const STRUCTURE_SUMMARY_LIMIT: usize = 200;
/// 回退确定性分组时每卷的章节数。
const FALLBACK_VOLUME_CHAPTERS: usize = 10;
/// 回退卷摘要取前几章。
const FALLBACK_SUMMARY_CHAPTERS: usize = 3;
/// 回退卷摘要每章的截断长度。
const FALLBACK_SUMMARY_CHARS: usize = 120;

/// 截断二分的最小粒度：低于该字符数不再拆分，直接记为跳过。
const MIN_SUBCHUNK_CHARS: usize = 800;
/// 截断二分递归深度上限。
const MAX_SUBCHUNK_DEPTH: usize = 3;

/// 单次分析调用的输出预算。推理模型的 reasoning tokens 与正文共享预算，
/// 必须显著高于普通沙盒调用的 1500，否则会以 finish_reason=length 截断。
const CHUNK_MAX_TOKENS: u32 = 12_000;
const STRUCTURE_MAX_TOKENS: u32 = 12_000;
/// 单次 LLM 尝试的墙钟上限；12k tokens 在推理模型上可能需要 100s+。
const IMPORT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(180);
/// 单次结构化调用的整体预算（含 repair 轮）。
const IMPORT_STEP_TIMEOUT_SECS: u64 = 240;
const IMPORT_SESSION_TIMEOUT_SECS: u64 = 3_600;

/// 后台任务注册表：取消令牌 + 并发闸（容量 2）。
pub struct ImportRegistry {
    handles: DashMap<String, CancellationToken>,
    sem: Semaphore,
    chain_override: Option<ProviderChain>,
}

impl ImportRegistry {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            handles: DashMap::new(),
            sem: Semaphore::new(max_concurrent.max(1)),
            chain_override: None,
        }
    }

    pub fn with_chain(max_concurrent: usize, chain: ProviderChain) -> Self {
        let mut registry = Self::new(max_concurrent);
        registry.chain_override = Some(chain);
        registry
    }

    pub fn register(&self, id: &str) -> CancellationToken {
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

    fn build_chain(&self, cfg: &AppConfig) -> ProviderChain {
        match &self.chain_override {
            Some(chain) => chain.clone(),
            None => factory::build_chain_with_attempt_timeout(cfg, IMPORT_ATTEMPT_TIMEOUT),
        }
    }
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: String,
    filename: String,
    size_bytes: i64,
    status: String,
    stage: String,
    progress: i64,
    chunks_total: i64,
    chunks_done: i64,
    message: Option<String>,
    error: Option<String>,
    novel_id: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl JobRow {
    fn into_dto(self) -> NovelImportJob {
        NovelImportJob {
            id: self.id,
            filename: self.filename,
            size_bytes: self.size_bytes,
            status: NovelImportStatus::from_db_or_default(&self.status),
            stage: NovelImportStage::from_db_or_default(&self.stage),
            progress: self.progress,
            chunks_total: self.chunks_total,
            chunks_done: self.chunks_done,
            message: self.message,
            error: self.error,
            novel_id: self.novel_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

const JOB_COLS: &str = "id, filename, size_bytes, status, stage, progress, chunks_total, \
                        chunks_done, message, error, novel_id, created_at, updated_at";

pub async fn load_job(pool: &SqlitePool, id: &str) -> Result<Option<NovelImportJob>, AppError> {
    let sql = format!("SELECT {JOB_COLS} FROM novel_imports WHERE id = ?");
    let row = sqlx::query_as::<_, JobRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(row.map(JobRow::into_dto))
}

pub async fn get_job(pool: &SqlitePool, id: &str) -> Result<NovelImportJob, AppError> {
    load_job(pool, id)
        .await?
        .ok_or_else(|| not_found(ErrorCode::ImportNotFound, "导入任务不存在"))
}

/// 创建任务并启动后台 runner（不阻塞请求）。
pub async fn create_job(
    pool: &SqlitePool,
    filename: &str,
    raw: Vec<u8>,
    registry: Arc<ImportRegistry>,
    cfg: AppConfig,
) -> Result<NovelImportJob, AppError> {
    let size_bytes = raw.len();
    if size_bytes > MAX_IMPORT_BYTES {
        return Err(AppError::too_large());
    }
    let text = String::from_utf8(raw)
        .map_err(|_| invalid(vec![("file", "必须是 UTF-8 文本".to_string())]))?;
    if text.trim().is_empty() {
        return Err(invalid(vec![("file", "文件不能为空".to_string())]));
    }
    let char_count = text.chars().count();
    if char_count < MIN_IMPORT_CHARS {
        return Err(invalid(vec![(
            "file",
            format!("文本至少需要 {MIN_IMPORT_CHARS} 个字符"),
        )]));
    }

    let filename = filename.trim();
    let filename = if filename.is_empty() {
        "novel.txt"
    } else {
        filename
    };
    let id: String = sqlx::query_scalar(
        "INSERT INTO novel_imports(filename, size_bytes, raw_text, status, stage, progress) \
         VALUES(?, ?, ?, 'queued', 'parsed', 0) RETURNING id",
    )
    .bind(filename)
    .bind(size_bytes as i64)
    .bind(&text)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;

    let job = load_job(pool, &id)
        .await?
        .ok_or_else(|| AppError::internal("导入任务创建后读取失败"))?;
    let token = registry.register(&id);
    let pool_bg = pool.clone();
    let registry_bg = registry.clone();
    let id_bg = id.clone();
    tokio::spawn(async move {
        run_job(pool_bg, id_bg, text, registry_bg, cfg, token).await;
    });
    Ok(job)
}

pub async fn cancel_job(
    pool: &SqlitePool,
    id: &str,
    registry: &ImportRegistry,
) -> Result<(), AppError> {
    // 行已不存在时同样视为「已丢弃」，DELETE 保持幂等。
    let Some(job) = load_job(pool, id).await? else {
        return Ok(());
    };
    if job.status.is_terminal() {
        // 终态任务视为「丢弃」：删除导入行（含 raw_text），幂等返回。
        // novel_id 是导入行指向小说的外键（ON DELETE SET NULL），删除子行不影响小说。
        sqlx::query(
            "DELETE FROM novel_imports WHERE id = ? AND status IN ('ready','failed','cancelled')",
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
        "UPDATE novel_imports SET status = 'cancelled', stage = 'failed', message = '用户已取消', \
         updated_at = strftime('%s','now') WHERE id = ? AND status IN ('queued','analyzing')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn get_preview(pool: &SqlitePool, id: &str) -> Result<NovelImportPreview, AppError> {
    let job = get_job(pool, id).await?;
    if job.status != NovelImportStatus::Ready {
        return Err(AppError::new(
            ErrorCode::ImportNotReady,
            "分析尚未完成，请稍后再试",
        ));
    }
    let raw: Option<String> =
        sqlx::query_scalar("SELECT analysis_json FROM novel_imports WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .flatten();
    let raw = raw.ok_or_else(|| AppError::internal("分析结果缺失"))?;
    let data: AnalysisData = serde_json::from_str(&raw)
        .map_err(|e| AppError::internal(format!("分析结果解析失败: {e}")))?;
    Ok(NovelImportPreview {
        job_id: job.id,
        title: data.title,
        world_view: data.world_view,
        background: data.background,
        summary: data.summary,
        volumes: data.volumes,
        characters: data.characters,
        relations: data.relations,
        items: data.items,
    })
}

// ============================================================
// 后台 runner
// ============================================================

enum RunError {
    Cancelled,
    Failed(String),
    /// 保留错误类型，供截断二分判断 InvalidOutput。
    Llm(LlmError),
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

async fn run_job(
    pool: SqlitePool,
    id: String,
    text: String,
    registry: Arc<ImportRegistry>,
    cfg: AppConfig,
    token: CancellationToken,
) {
    let outcome = run_job_inner(&pool, &id, &text, &registry, &cfg, &token).await;
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
        Err(RunError::Llm(error)) => {
            if token.is_cancelled() {
                mark_cancelled(&pool, &id).await
            } else {
                mark_failed(&pool, &id, &format!("LLM 分析失败: {}", error.message)).await
            }
        }
    };
    if let Err(e) = result {
        tracing::warn!(import_id = %id, error = %e, "novel import terminal status write failed");
    }
}

async fn mark_cancelled(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE novel_imports SET status = 'cancelled', stage = 'failed', message = '用户已取消', \
         updated_at = strftime('%s','now') WHERE id = ? AND status IN ('queued','analyzing')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn mark_failed(pool: &SqlitePool, id: &str, message: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE novel_imports SET status = 'failed', stage = 'failed', error = ?, \
         updated_at = strftime('%s','now') WHERE id = ? AND status = 'analyzing'",
    )
    .bind(message)
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn run_job_inner(
    pool: &SqlitePool,
    id: &str,
    text: &str,
    registry: &ImportRegistry,
    cfg: &AppConfig,
    token: &CancellationToken,
) -> Result<(), RunError> {
    let permit = tokio::select! {
        permit = registry.sem.acquire() => permit.map_err(|_| RunError::Failed("分析并发闸已关闭".to_string()))?,
        _ = token.cancelled() => return Err(RunError::Cancelled),
    };
    let _permit = permit;

    let affected = sqlx::query(
        "UPDATE novel_imports SET status = 'analyzing', stage = 'chunking', progress = 5, \
         error = NULL, updated_at = strftime('%s','now') WHERE id = ? AND status = 'queued'",
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
    let chain = registry.build_chain(cfg);
    if chain.is_empty() {
        return Err(RunError::Failed(
            "未配置 LLM API Key，无法分析小说".to_string(),
        ));
    }
    let llm = Arc::new(LlmService::new(chain, pool.clone(), None));

    analyze(pool, id, text, llm, token).await
}

async fn analyze(
    pool: &SqlitePool,
    id: &str,
    text: &str,
    llm: Arc<LlmService>,
    token: &CancellationToken,
) -> Result<(), RunError> {
    let chapters = split_into_chapters(text);
    if chapters.is_empty() {
        return Err(RunError::Failed("文本无法解析出章节".to_string()));
    }
    let chunks = chunk_chapters(&chapters).map_err(|e| RunError::Failed(e.message))?;
    let total = chunks.len() as i64;
    let mut note = None;

    sqlx::query(
        "UPDATE novel_imports SET chunks_total = ?, stage = 'analyzing_chunks', progress = 10, \
         message = ?, updated_at = strftime('%s','now') WHERE id = ? AND status = 'analyzing'",
    )
    .bind(total)
    .bind(&note)
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_failure)?;

    let outcomes = analyze_chunks(pool, id, llm.clone(), &chunks, total, token).await?;
    let (chunk_characters, chunk_items, chunk_relations, digests, skipped_segments) =
        collect_chunk_outcomes(&chunks, outcomes);

    if skipped_segments > 0 {
        append_note(
            &mut note,
            format!("其中 {skipped_segments} 段未能解析（内容过于密集）"),
        );
    }

    ensure_active(token)?;
    sqlx::query(
        "UPDATE novel_imports SET stage = 'merging', progress = 85, updated_at = strftime('%s','now') \
         WHERE id = ? AND status = 'analyzing'",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_failure)?;

    let characters = merge_characters(chunk_characters);
    let items = merge_items(chunk_items);
    let relations = merge_relations(chunk_relations);

    ensure_active(token)?;
    sqlx::query(
        "UPDATE novel_imports SET stage = 'structuring', progress = 90, \
         updated_at = strftime('%s','now') WHERE id = ? AND status = 'analyzing'",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_failure)?;

    let assembled = assemble_chapters(&chapters, &digests);
    let (body_chapters, preface_chapter) = split_preface(&assembled);
    let structure_summaries: Vec<String> = body_chapters
        .iter()
        .map(|chapter| structure_summary(chapter))
        .collect();
    let structure = generate_structure(llm.as_ref(), &structure_summaries, token).await?;
    let plans = validated_volume_plans(&structure.volumes, structure_summaries.len())
        .unwrap_or_else(|| deterministic_volume_plans(&structure_summaries));
    let volumes = build_volumes(&plans, &body_chapters, preface_chapter);
    let beats = assembled
        .iter()
        .flat_map(|chapter| chapter.beats.iter().cloned())
        .collect();

    ensure_active(token)?;
    let data = AnalysisData {
        title: structure.title,
        world_view: structure.world_view,
        background: structure.background,
        summary: structure.summary,
        volumes,
        beats,
        characters,
        relations,
        items,
    };
    let analysis_json = serde_json::to_string(&data)
        .map_err(|e| RunError::Failed(format!("分析结果序列化失败: {e}")))?;

    if note.is_none() {
        note = truncation_note(&data);
    }
    sqlx::query(
        "UPDATE novel_imports SET status = 'ready', stage = 'ready', progress = 100, \
         chunks_done = chunks_total, analysis_json = ?, message = COALESCE(?, message), \
         error = NULL, updated_at = strftime('%s','now') WHERE id = ? AND status = 'analyzing'",
    )
    .bind(&analysis_json)
    .bind(&note)
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_failure)?;
    Ok(())
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

enum ChunkOutcome {
    Parsed(ChunkOutput),
    Skipped,
}

/// 并发分析所有分块，结果按下标回填；任一分块出现非截断错误时立即中止。
async fn analyze_chunks(
    pool: &SqlitePool,
    id: &str,
    llm: Arc<LlmService>,
    chunks: &[ChapterChunk],
    total: i64,
    token: &CancellationToken,
) -> Result<Vec<Option<ChunkOutcome>>, RunError> {
    let semaphore = Arc::new(Semaphore::new(IMPORT_CHUNK_CONCURRENCY));
    let completed = Arc::new(AtomicUsize::new(0));
    let mut outcomes: Vec<Option<ChunkOutcome>> = (0..chunks.len()).map(|_| None).collect();
    let mut tasks: JoinSet<(usize, Result<ChunkOutcome, RunError>)> = JoinSet::new();
    let mut next_index = 0usize;

    while next_index < chunks.len() || !tasks.is_empty() {
        if token.is_cancelled() {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            return Err(RunError::Cancelled);
        }

        while next_index < chunks.len() {
            if token.is_cancelled() {
                break;
            }
            let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                break;
            };
            let chunk = chunks[next_index].clone();
            let index = next_index;
            let llm = llm.clone();
            let token = token.clone();
            tasks.spawn(async move {
                let _permit = permit;
                let outcome = analyze_chunk_text(&llm, index, total, &chunk.text, &token, 0).await;
                (index, outcome)
            });
            next_index += 1;
        }

        let joined = tokio::select! {
            _ = token.cancelled() => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                return Err(RunError::Cancelled);
            }
            joined = tasks.join_next() => joined,
        };
        let Some(joined) = joined else {
            break;
        };
        let (index, outcome) =
            joined.map_err(|e| RunError::Failed(format!("分块任务异常终止: {e}")))?;

        match outcome {
            Ok(outcome) => outcomes[index] = Some(outcome),
            Err(error) => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                return Err(error);
            }
        }

        // 只有协调任务写库；原子计数保证 chunks_done 单调递增。
        let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
        let progress = (10 + (70 * done as i64) / total).clamp(10, 80);
        sqlx::query(
            "UPDATE novel_imports SET chunks_done = ?, progress = ?, \
             updated_at = strftime('%s','now') WHERE id = ? AND status = 'analyzing'",
        )
        .bind(done as i64)
        .bind(progress)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_failure)?;
    }

    Ok(outcomes)
}

/// 对单个分块做分析；输出被截断时二分递归，最小粒度仍失败则跳过该段。
async fn analyze_chunk_text(
    llm: &LlmService,
    index: usize,
    total: i64,
    text: &str,
    token: &CancellationToken,
    depth: usize,
) -> Result<ChunkOutcome, RunError> {
    match call_chunk(llm, index, total, text, token).await {
        Ok(output) => Ok(ChunkOutcome::Parsed(output)),
        Err(RunError::Llm(error))
            if matches!(
                error.kind,
                LlmErrorKind::InvalidOutput | LlmErrorKind::Truncated
            ) =>
        {
            if depth >= MAX_SUBCHUNK_DEPTH || text.chars().count() <= MIN_SUBCHUNK_CHARS {
                return Ok(ChunkOutcome::Skipped);
            }
            let Some((left, right)) = split_half_at_boundary(text) else {
                return Ok(ChunkOutcome::Skipped);
            };
            let left = Box::pin(analyze_chunk_text(
                llm,
                index,
                total,
                &left,
                token,
                depth + 1,
            ))
            .await?;
            let right = Box::pin(analyze_chunk_text(
                llm,
                index,
                total,
                &right,
                token,
                depth + 1,
            ))
            .await?;
            Ok(merge_chunk_outcomes(left, right))
        }
        Err(error) => Err(error),
    }
}

async fn call_chunk(
    llm: &LlmService,
    index: usize,
    total: i64,
    text: &str,
    token: &CancellationToken,
) -> Result<ChunkOutput, RunError> {
    let system = prompts::render(
        prompts::NOVEL_ANALYZE_CHUNK,
        &json!({ "chunk_index": index + 1, "chunk_total": total }),
    )
    .map_err(|e| RunError::Failed(e.message))?;
    let messages = vec![Message::system(system), Message::user(text.to_string())];
    let value = structured_with_retry(
        llm,
        Purpose::Analyze,
        messages,
        schemas::novel_chunk_schema(),
        CHUNK_MAX_TOKENS,
        token,
    )
    .await?;
    serde_json::from_value(value).map_err(|e| RunError::Failed(format!("分块结果解析失败: {e}")))
}

fn merge_chunk_outcomes(left: ChunkOutcome, right: ChunkOutcome) -> ChunkOutcome {
    match (left, right) {
        (ChunkOutcome::Skipped, ChunkOutcome::Skipped) => ChunkOutcome::Skipped,
        (ChunkOutcome::Skipped, ChunkOutcome::Parsed(output))
        | (ChunkOutcome::Parsed(output), ChunkOutcome::Skipped) => ChunkOutcome::Parsed(output),
        (ChunkOutcome::Parsed(left), ChunkOutcome::Parsed(right)) => {
            ChunkOutcome::Parsed(ChunkOutput {
                summary: format!("{}\n{}", left.summary, right.summary),
                beats: left.beats.into_iter().chain(right.beats).collect(),
                characters: left
                    .characters
                    .into_iter()
                    .chain(right.characters)
                    .collect(),
                items: left.items.into_iter().chain(right.items).collect(),
                relations: left.relations.into_iter().chain(right.relations).collect(),
            })
        }
    }
}

#[derive(Default, Clone)]
struct ChapterDigest {
    summary: String,
    beats: Vec<String>,
}

struct AssembledChapter {
    source_index: usize,
    title: String,
    outline: String,
    summary: String,
    beats: Vec<String>,
}

type CollectedChunks = (
    Vec<PreviewCharacter>,
    Vec<PreviewItem>,
    Vec<PreviewRelation>,
    HashMap<usize, ChapterDigest>,
    usize,
);

fn collect_chunk_outcomes(
    chunks: &[ChapterChunk],
    outcomes: Vec<Option<ChunkOutcome>>,
) -> CollectedChunks {
    let mut characters = Vec::new();
    let mut items = Vec::new();
    let mut relations = Vec::new();
    let mut digests: HashMap<usize, ChapterDigest> = HashMap::new();
    let mut skipped = 0usize;
    for (chunk, outcome) in chunks.iter().zip(outcomes) {
        let Some(outcome) = outcome else {
            continue;
        };
        match outcome {
            ChunkOutcome::Parsed(output) => {
                characters.extend(output.characters);
                items.extend(output.items);
                relations.extend(output.relations);
                let digest = digests.entry(chunk.chapter_index).or_default();
                append_summary(&mut digest.summary, &output.summary);
                digest.beats.extend(output.beats);
            }
            ChunkOutcome::Skipped => skipped += 1,
        }
    }
    (characters, items, relations, digests, skipped)
}

fn append_summary(target: &mut String, incoming: &str) {
    let incoming = incoming.trim();
    if incoming.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(incoming);
}

fn assemble_chapters(
    chapters: &[SourceChapter],
    digests: &HashMap<usize, ChapterDigest>,
) -> Vec<AssembledChapter> {
    chapters
        .iter()
        .map(|chapter| {
            let digest = digests.get(&chapter.index);
            let summary = digest
                .map(|digest| clip_text(digest.summary.trim(), SUMMARY_JOIN_LIMIT))
                .unwrap_or_default();
            let beats = dedup_beats(
                digest
                    .map(|digest| digest.beats.as_slice())
                    .unwrap_or_default(),
            );
            let outline = build_chapter_outline(&summary, &beats);
            AssembledChapter {
                source_index: chapter.index,
                title: chapter.title.clone(),
                outline,
                summary,
                beats,
            }
        })
        .collect()
}

fn dedup_beats(raw: &[String]) -> Vec<String> {
    let mut beats: Vec<String> = Vec::new();
    for beat in raw {
        let beat = beat.trim();
        if beat.is_empty() || beats.iter().any(|existing| existing.as_str() == beat) {
            continue;
        }
        beats.push(beat.to_string());
        if beats.len() == CHAPTER_BEATS_LIMIT {
            break;
        }
    }
    beats
}

fn build_chapter_outline(summary: &str, beats: &[String]) -> String {
    if summary.is_empty() && beats.is_empty() {
        return String::new();
    }
    let mut outline = String::from("## 剧情梗概\n");
    outline.push_str(summary);
    outline.push_str("\n## 关键场景\n");
    for beat in beats {
        outline.push_str("- ");
        outline.push_str(beat);
        outline.push('\n');
    }
    outline.trim_end().to_string()
}

fn structure_summary(chapter: &AssembledChapter) -> String {
    let raw = if chapter.summary.trim().is_empty() {
        chapter.title.trim()
    } else {
        chapter.summary.trim()
    };
    clip_text(raw, STRUCTURE_SUMMARY_LIMIT)
}

fn split_preface(
    assembled: &[AssembledChapter],
) -> (Vec<&AssembledChapter>, Option<&AssembledChapter>) {
    if assembled.iter().any(|chapter| chapter.source_index > 0) {
        (
            assembled
                .iter()
                .filter(|chapter| chapter.source_index > 0)
                .collect(),
            assembled.iter().find(|chapter| chapter.source_index == 0),
        )
    } else {
        (assembled.iter().collect(), None)
    }
}

fn validated_volume_plans(plans: &[VolumePlan], total_chapters: usize) -> Option<Vec<VolumePlan>> {
    if plans.is_empty() || total_chapters == 0 {
        return None;
    }
    let mut expected_start = 1usize;
    for plan in plans {
        if plan.start != expected_start || plan.end < plan.start || plan.end > total_chapters {
            return None;
        }
        if plan.end - plan.start + 1 > 30 {
            return None;
        }
        expected_start = plan.end + 1;
    }
    if expected_start != total_chapters + 1 {
        return None;
    }
    Some(plans.to_vec())
}

fn deterministic_volume_plans(summaries: &[String]) -> Vec<VolumePlan> {
    if summaries.is_empty() {
        return Vec::new();
    }
    summaries
        .chunks(FALLBACK_VOLUME_CHAPTERS)
        .enumerate()
        .map(|(index, group)| {
            let start = index * FALLBACK_VOLUME_CHAPTERS + 1;
            let end = start + group.len() - 1;
            let summary = group
                .iter()
                .take(FALLBACK_SUMMARY_CHAPTERS)
                .map(|item| clip_text(item.trim(), FALLBACK_SUMMARY_CHARS))
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
                .join("；");
            VolumePlan {
                start,
                end,
                title: format!("卷{}", index + 1),
                summary: non_empty(summary),
            }
        })
        .collect()
}

fn build_volumes(
    plans: &[VolumePlan],
    body_chapters: &[&AssembledChapter],
    preface: Option<&AssembledChapter>,
) -> Vec<PreviewVolume> {
    plans
        .iter()
        .enumerate()
        .map(|(volume_index, plan)| {
            let mut chapters = Vec::new();
            if volume_index == 0 {
                if let Some(preface) = preface {
                    chapters.push(preview_chapter(preface));
                }
            }
            for chapter_number in plan.start..=plan.end {
                if let Some(chapter) = body_chapters.get(chapter_number - 1) {
                    chapters.push(preview_chapter(chapter));
                }
            }
            let description = strip_ordinal_prefix(plan.title.trim()).trim();
            PreviewVolume {
                title: ordinal_title("卷", volume_index + 1, description),
                summary: plan.summary.clone().and_then(non_empty),
                chapters,
            }
        })
        .collect()
}

fn preview_chapter(chapter: &AssembledChapter) -> PreviewChapter {
    PreviewChapter {
        title: chapter.title.clone(),
        outline: chapter.outline.clone(),
    }
}

#[derive(Default)]
struct StructureAccumulator {
    title: Option<String>,
    world_view: Option<String>,
    background: Option<String>,
    summary: Option<String>,
    volumes: Vec<VolumePlan>,
}

impl StructureAccumulator {
    fn absorb(&mut self, output: StructureOutput) {
        let title = non_empty(output.title);
        fill_option(&mut self.title, &title);
        fill_option(&mut self.world_view, &output.world_view);
        fill_option(&mut self.background, &output.background);
        fill_option(&mut self.summary, &output.summary);
        self.volumes.extend(output.volumes);
    }

    /// 中间结果：保留空标题，避免回退值污染后续批次的「首个非空值」。
    fn into_output(self) -> StructureOutput {
        StructureOutput {
            title: self.title.unwrap_or_default(),
            world_view: self.world_view,
            background: self.background,
            summary: self.summary,
            volumes: self.volumes,
        }
    }

    fn finish(self) -> StructureOutput {
        let StructureOutput {
            mut title,
            world_view,
            background,
            summary,
            volumes,
        } = self.into_output();
        if title.trim().is_empty() {
            title = "未命名作品".to_string();
        }
        StructureOutput {
            title,
            world_view: Some(world_view.unwrap_or_default()),
            background: Some(background.unwrap_or_default()),
            summary: Some(summary.unwrap_or_default()),
            volumes,
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

/// 分批生成结构；单批截断时二分该批章节区间，最终按批次顺序拼接。
async fn generate_structure(
    llm: &LlmService,
    summaries: &[String],
    token: &CancellationToken,
) -> Result<StructureOutput, RunError> {
    if summaries.is_empty() {
        return Ok(StructureOutput::default());
    }
    let clipped: Vec<String> = summaries
        .iter()
        .map(|summary| clip_text(summary.trim(), STRUCTURE_SUMMARY_LIMIT))
        .collect();
    let mut accumulator = StructureAccumulator::default();
    let mut start = 1usize;
    while start <= clipped.len() {
        let end = (start + STRUCTURE_BATCH_SIZE - 1).min(clipped.len());
        let output = structure_batch(llm, &clipped, start, end, token).await?;
        accumulator.absorb(output);
        start = end + 1;
    }
    Ok(accumulator.finish())
}

async fn structure_batch(
    llm: &LlmService,
    summaries: &[String],
    start: usize,
    end: usize,
    token: &CancellationToken,
) -> Result<StructureOutput, RunError> {
    ensure_active(token)?;
    let batch = &summaries[start - 1..end];
    let numbered = numbered_summaries(batch, start);
    let system = prompts::render(
        prompts::NOVEL_ANALYZE_STRUCTURE,
        &json!({
            "chapters": numbered,
            "chapter_count": batch.len(),
            "range_start": start,
            "range_end": end,
            "total_chapters": summaries.len(),
        }),
    )
    .map_err(|e| RunError::Failed(e.message))?;
    let value = match structured_with_retry(
        llm,
        Purpose::Structure,
        vec![Message::system(system)],
        schemas::novel_structure_schema(),
        STRUCTURE_MAX_TOKENS,
        token,
    )
    .await
    {
        Ok(value) => value,
        Err(RunError::Llm(error))
            if matches!(
                error.kind,
                LlmErrorKind::InvalidOutput | LlmErrorKind::Truncated
            ) =>
        {
            return split_structure_batch(llm, summaries, start, end, token).await;
        }
        Err(error) => return Err(error),
    };
    serde_json::from_value(value).map_err(|e| RunError::Failed(format!("结构结果解析失败: {e}")))
}

/// 截断区间的二分处理；单章仍失败时返回空结构，交由确定性分组兜底。
async fn split_structure_batch(
    llm: &LlmService,
    summaries: &[String],
    start: usize,
    end: usize,
    token: &CancellationToken,
) -> Result<StructureOutput, RunError> {
    if start >= end {
        return Ok(StructureOutput::default());
    }
    let middle = start + (end - start) / 2;
    let mut accumulator = StructureAccumulator::default();
    accumulator.absorb(Box::pin(structure_batch(llm, summaries, start, middle, token)).await?);
    accumulator.absorb(Box::pin(structure_batch(llm, summaries, middle + 1, end, token)).await?);
    Ok(accumulator.into_output())
}

fn truncation_note(data: &AnalysisData) -> Option<String> {
    let too_long = data.title.chars().count() > 200
        || data
            .summary
            .as_deref()
            .is_some_and(|v| v.chars().count() > 20000);
    if too_long {
        Some("部分字段超长，创建项目时将自动截断".to_string())
    } else {
        None
    }
}

async fn structured_with_retry(
    llm: &LlmService,
    purpose: Purpose,
    messages: Vec<Message>,
    schema: Value,
    max_tokens: u32,
    token: &CancellationToken,
) -> Result<Value, RunError> {
    let mut last_error: Option<LlmError> = None;
    for attempt in 0..=ANALYSIS_RETRIES {
        ensure_active(token)?;
        let budget = Budget::new(
            IMPORT_SESSION_TIMEOUT_SECS,
            IMPORT_STEP_TIMEOUT_SECS,
            IMPORT_ATTEMPT_TIMEOUT.as_secs(),
        );
        match llm
            .complete_structured_with_tokens(
                &budget,
                purpose,
                messages.clone(),
                schema.clone(),
                2,
                max_tokens,
            )
            .await
        {
            Ok((value, _)) => return Ok(value),
            Err(error) => {
                // 截断/解析类错误对相同输入是确定性的，重试无意义；
                // 认证/请求错误同样不应重试。只对瞬时错误做有限重试。
                if !error.retryable() {
                    return Err(RunError::Llm(error));
                }
                last_error = Some(error);
                if attempt < ANALYSIS_RETRIES {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                        _ = token.cancelled() => return Err(RunError::Cancelled),
                    }
                }
            }
        }
    }
    let error =
        last_error.unwrap_or_else(|| LlmError::new(LlmErrorKind::Network, "Provider 链未返回结果"));
    Err(RunError::Llm(error))
}

#[derive(Deserialize)]
struct ChunkOutput {
    summary: String,
    #[serde(default)]
    beats: Vec<String>,
    #[serde(default)]
    characters: Vec<PreviewCharacter>,
    #[serde(default)]
    items: Vec<PreviewItem>,
    #[serde(default)]
    relations: Vec<PreviewRelation>,
}

#[derive(Default, Clone, Deserialize)]
struct VolumePlan {
    #[serde(default)]
    start: usize,
    #[serde(default)]
    end: usize,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Default, Deserialize)]
struct StructureOutput {
    title: String,
    #[serde(default)]
    world_view: Option<String>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    volumes: Vec<VolumePlan>,
}

// ============================================================
// 截断二分 / 分批（纯函数，便于单测）
// ============================================================

/// 按字符数中点把文本切成两半；保证两侧非空且都短于原串。
pub fn split_half_at_boundary(text: &str) -> Option<(String, String)> {
    let char_count = text.chars().count();
    if char_count < 2 {
        return None;
    }
    let split_at = text
        .char_indices()
        .nth(char_count / 2)
        .map(|(index, _)| index)?;
    let (left, right) = text.split_at(split_at);
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left.to_string(), right.to_string()))
}

/// 按固定大小切批；空输入或非法批次大小返回空列表。
pub fn split_into_batches<T: Clone>(items: &[T], batch_size: usize) -> Vec<Vec<T>> {
    if batch_size == 0 {
        return Vec::new();
    }
    items.chunks(batch_size).map(<[T]>::to_vec).collect()
}

fn is_ordinal_numeral(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(
            c,
            '一' | '二'
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
                | '万'
                | '零'
                | '〇'
                | '两'
        )
}

/// 去掉标题开头的「第X卷/章/回/节」序号及其后的空白与分隔符。
pub fn strip_ordinal_prefix(title: &str) -> &str {
    let trimmed = title.trim_start();
    let Some(rest) = trimmed.strip_prefix('第') else {
        return title;
    };

    let mut cursor = skip_whitespace(rest, 0);
    let numeral_start = cursor;
    cursor = skip_while(rest, cursor, is_ordinal_numeral);
    if cursor == numeral_start {
        return title;
    }
    cursor = skip_whitespace(rest, cursor);

    let Some(suffix) = rest[cursor..].chars().next() else {
        return title;
    };
    if !matches!(suffix, '卷' | '章' | '回' | '节') {
        return title;
    }
    cursor += suffix.len_utf8();

    while let Some(c) = rest[cursor..].chars().next() {
        if !c.is_whitespace() && !matches!(c, '、' | '：' | ':' | '·' | '-' | '—') {
            break;
        }
        cursor += c.len_utf8();
    }
    &rest[cursor..]
}

fn skip_whitespace(text: &str, mut cursor: usize) -> usize {
    while let Some(c) = text[cursor..].chars().next() {
        if !c.is_whitespace() {
            break;
        }
        cursor += c.len_utf8();
    }
    cursor
}

fn skip_while(text: &str, mut cursor: usize, predicate: fn(char) -> bool) -> usize {
    while let Some(c) = text[cursor..].chars().next() {
        if !predicate(c) {
            break;
        }
        cursor += c.len_utf8();
    }
    cursor
}

/// 1..=99 的中文数字；超出范围时回退为阿拉伯数字，避免 panic。
pub fn chinese_numeral(n: usize) -> String {
    const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if n == 0 {
        return DIGITS[0].to_string();
    }
    if n > 99 {
        return n.to_string();
    }
    if n < 10 {
        return DIGITS[n].to_string();
    }
    if n == 10 {
        return "十".to_string();
    }
    let tens = n / 10;
    let ones = n % 10;
    if n < 20 {
        return format!("十{}", DIGITS[ones]);
    }
    if ones == 0 {
        format!("{}十", DIGITS[tens])
    } else {
        format!("{}十{}", DIGITS[tens], DIGITS[ones])
    }
}

fn ordinal_title(unit: &str, index: usize, description: &str) -> String {
    let prefix = format!("第{}{}", chinese_numeral(index), unit);
    if description.is_empty() {
        prefix
    } else {
        format!("{prefix} {description}")
    }
}

// ============================================================
// 原文解析 / 分块（章节边界由原文决定）
// ============================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChapter {
    pub index: usize,
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterChunk {
    pub chapter_index: usize,
    pub text: String,
}

fn is_chapter_numeral(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(
            c,
            '零' | '〇'
                | '一'
                | '二'
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
                | '万'
                | '两'
        )
}

fn is_chinese_chapter_heading(content: &str) -> bool {
    let Some(rest) = content.strip_prefix('第') else {
        return false;
    };
    let rest = rest.trim_start();
    let mut numeral_end = 0usize;
    for (index, c) in rest.char_indices() {
        if !is_chapter_numeral(c) {
            break;
        }
        numeral_end = index + c.len_utf8();
    }
    if numeral_end == 0 {
        return false;
    }
    let tail = rest[numeral_end..].trim_start();
    matches!(tail.chars().next(), Some('章' | '回' | '节'))
}

fn is_english_chapter_heading(content: &str) -> bool {
    let lowered = content.to_lowercase();
    let Some(rest) = lowered.strip_prefix("chapter") else {
        return false;
    };
    let digits = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .count();
    (1..=8).contains(&digits)
}

/// 识别章节标题行；返回去除 Markdown `#` 与首尾空白后的原始标题。
fn parse_chapter_heading(line: &str) -> Option<String> {
    let content = line.trim().trim_start_matches('#').trim_start();
    if content.is_empty() {
        return None;
    }
    if !is_chinese_chapter_heading(content) && !is_english_chapter_heading(content) {
        return None;
    }
    Some(content.trim_end().to_string())
}

fn push_source_chapter(
    chapters: &mut Vec<SourceChapter>,
    next_index: &mut usize,
    title: String,
    text: String,
) {
    chapters.push(SourceChapter {
        index: *next_index,
        title,
        text,
    });
    *next_index += 1;
}

/// 按原文标题行切分章节，章节边界不再交给 LLM。
pub fn split_into_chapters(raw: &str) -> Vec<SourceChapter> {
    let mut chapters = Vec::new();
    let mut next_index = 1usize;
    let mut preface = String::new();
    let mut current: Option<(String, String)> = None;

    for line in raw.split_inclusive('\n') {
        if let Some(title) = parse_chapter_heading(line) {
            if let Some((title, text)) = current.take() {
                push_source_chapter(&mut chapters, &mut next_index, title, text);
            } else if preface.trim().chars().count() >= MIN_IMPORT_CHARS {
                chapters.push(SourceChapter {
                    index: 0,
                    title: "前言".to_string(),
                    text: std::mem::take(&mut preface),
                });
            } else {
                preface.clear();
            }
            current = Some((title, String::new()));
            continue;
        }
        match current.as_mut() {
            Some((_, text)) => text.push_str(line),
            None => preface.push_str(line),
        }
    }

    match current {
        Some((title, text)) => push_source_chapter(&mut chapters, &mut next_index, title, text),
        None if preface.trim().chars().count() >= MIN_IMPORT_CHARS => {
            chapters.push(SourceChapter {
                index: 0,
                title: "前言".to_string(),
                text: preface,
            })
        }
        None => {}
    }

    chapters
}

/// 每个章节独立分块；长章节按现有重叠窗口切分，短章节绝不跨章合并。
pub fn chunk_chapters(chapters: &[SourceChapter]) -> Result<Vec<ChapterChunk>, AppError> {
    let mut chunks = Vec::new();
    for chapter in chapters {
        let text = chapter.text.trim();
        if text.is_empty() {
            continue;
        }
        let (parts, _) = chunk_segment(text, usize::MAX);
        for part in parts {
            chunks.push(ChapterChunk {
                chapter_index: chapter.index,
                text: part,
            });
            if chunks.len() > MAX_CHUNKS {
                return Err(AppError::validation(
                    "文本过长，请拆分为多部后分别导入",
                    json!({}),
                ));
            }
        }
    }
    Ok(chunks)
}

fn char_boundary_after(text: &str, start: usize, count: usize) -> usize {
    let mut boundary = text.len();
    for (taken, (index, c)) in text[start..].char_indices().enumerate() {
        if taken + 1 == count {
            boundary = start + index + c.len_utf8();
            break;
        }
    }
    boundary
}

fn backtrack_chars(text: &str, start: usize, end: usize, overlap: usize) -> usize {
    if overlap == 0 {
        return end;
    }
    text[start..end]
        .char_indices()
        .rev()
        .nth(overlap - 1)
        .map(|(index, _)| start + index)
        .unwrap_or(start)
}

fn chunk_segment(segment: &str, limit: usize) -> (Vec<String>, bool) {
    let mut chunks = Vec::new();
    if limit == 0 {
        return (chunks, !segment.is_empty());
    }
    let mut start = 0usize;
    loop {
        if start >= segment.len() {
            break;
        }
        let has_more = segment[start..].chars().nth(CHUNK_CHARS).is_some();
        let end = if has_more {
            char_boundary_after(segment, start, CHUNK_CHARS)
        } else {
            segment.len()
        };
        let more_after = end < segment.len();
        chunks.push(segment[start..end].to_string());
        if !more_after {
            break;
        }
        if chunks.len() >= limit {
            return (chunks, true);
        }
        let next = backtrack_chars(segment, start, end, CHUNK_OVERLAP);
        start = if next <= start { end } else { next };
    }
    (chunks, false)
}

// ============================================================
// 合并（人物/道具/关系）
// ============================================================

fn filled(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.trim().is_empty())
}

fn fill_option(target: &mut Option<String>, source: &Option<String>) {
    if !filled(target) && filled(source) {
        *target = source.clone();
    }
}

fn role_rank(role: &str) -> u8 {
    match role {
        "protagonist" => 3,
        "deuteragonist" => 2,
        "antagonist" => 1,
        _ => 0,
    }
}

pub fn merge_characters(input: Vec<PreviewCharacter>) -> Vec<PreviewCharacter> {
    let mut merged: Vec<PreviewCharacter> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for mut character in input {
        character.name = character.name.trim().to_string();
        if character.name.is_empty() {
            continue;
        }
        let key = character.name.to_lowercase();
        let Some(&position) = index.get(&key) else {
            index.insert(key, merged.len());
            merged.push(character);
            continue;
        };
        let existing = &mut merged[position];
        let incoming_biography = character.biography.as_deref().unwrap_or("").chars().count();
        let existing_biography = existing.biography.as_deref().unwrap_or("").chars().count();
        if filled(&character.biography) && incoming_biography > existing_biography {
            existing.biography = character.biography.clone();
        }
        fill_option(&mut existing.appearance, &character.appearance);
        fill_option(&mut existing.personality, &character.personality);
        fill_option(
            &mut existing.hidden_personality,
            &character.hidden_personality,
        );
        fill_option(&mut existing.current_goal, &character.current_goal);
        fill_option(&mut existing.speaking_style, &character.speaking_style);
        if role_rank(&character.role_type) > role_rank(&existing.role_type) {
            existing.role_type = character.role_type;
        }
    }
    merged
}

pub fn merge_items(input: Vec<PreviewItem>) -> Vec<PreviewItem> {
    let mut merged: Vec<PreviewItem> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for mut item in input {
        item.name = item.name.trim().to_string();
        if item.name.is_empty() {
            continue;
        }
        let key = item.name.to_lowercase();
        let Some(&position) = index.get(&key) else {
            index.insert(key, merged.len());
            merged.push(item);
            continue;
        };
        let existing = &mut merged[position];
        fill_option(&mut existing.description, &item.description);
        fill_option(&mut existing.features, &item.features);
        fill_option(&mut existing.owner, &item.owner);
        existing.importance = existing.importance.max(item.importance);
    }
    merged
}

pub fn merge_relations(input: Vec<PreviewRelation>) -> Vec<PreviewRelation> {
    let mut merged: Vec<PreviewRelation> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for mut relation in input {
        let a = relation.a.trim().to_string();
        let b = relation.b.trim().to_string();
        if a.is_empty() || b.is_empty() || a.to_lowercase() == b.to_lowercase() {
            continue;
        }
        relation.a = a;
        relation.b = b;
        let mut key = (relation.a.to_lowercase(), relation.b.to_lowercase());
        if key.0 > key.1 {
            key = (key.1, key.0);
        }
        let Some(&position) = index.get(&key) else {
            index.insert(key, merged.len());
            merged.push(relation);
            continue;
        };
        let existing = &mut merged[position];
        let first_conflict = if filled(&existing.conflict_desc) {
            existing.conflict_desc.clone()
        } else {
            relation.conflict_desc.clone()
        };
        if relation.affinity.abs() > existing.affinity.abs() {
            let mut replacement = relation;
            replacement.conflict_desc = first_conflict;
            *existing = replacement;
        } else if !filled(&existing.conflict_desc) {
            existing.conflict_desc = relation.conflict_desc;
        }
    }
    merged
}

// ============================================================
// 摘要归并
// ============================================================

fn clip_text(raw: &str, max: usize) -> String {
    if raw.chars().count() <= max {
        return raw.to_string();
    }
    raw.chars().take(max).collect()
}

fn joined_summary_len(items: &[String]) -> usize {
    items.iter().map(|item| item.chars().count() + 8).sum()
}

/// 每 20 条摘要合并为 1 条（截断至 300 字），递归直到拼接长度 ≤ 12000。
pub fn layer_summaries(input: &[String]) -> Vec<String> {
    let mut current: Vec<String> = input
        .iter()
        .map(|summary| clip_text(summary, SUMMARY_ITEM_LIMIT))
        .collect();
    while joined_summary_len(&current) > SUMMARY_JOIN_LIMIT && current.len() > 1 {
        let next: Vec<String> = current
            .chunks(SUMMARY_MERGE_GROUP)
            .map(|group| clip_text(&group.join("；"), SUMMARY_ITEM_LIMIT))
            .collect();
        if next.len() >= current.len() {
            break;
        }
        current = next;
    }
    current
}

fn numbered_summaries(items: &[String], start: usize) -> String {
    items
        .iter()
        .enumerate()
        .map(|(index, summary)| format!("{}. {}", start + index, summary))
        .collect::<Vec<_>>()
        .join("\n")
}

// ============================================================
// 创建项目（单事务）
// ============================================================

fn clip_field(value: &str, max: usize) -> (String, bool) {
    if value.chars().count() <= max {
        (value.to_string(), false)
    } else {
        (value.chars().take(max).collect(), true)
    }
}

fn clip_optional(value: &Option<String>, max: usize) -> (Option<String>, bool) {
    match value {
        Some(raw) => {
            let (clipped, truncated) = clip_field(raw, max);
            (Some(clipped), truncated)
        }
        None => (None, false),
    }
}

fn file_stem(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
        .to_string()
}

pub async fn create_project(
    pool: &SqlitePool,
    job_id: &str,
    req: CreateNovelFromImportReq,
) -> Result<crate::models::dto::NovelDto, AppError> {
    let job = get_job(pool, job_id).await?;
    if job.status != NovelImportStatus::Ready || job.novel_id.is_some() {
        return Err(AppError::new(
            ErrorCode::ImportConflict,
            "任务未就绪或已生成项目",
        ));
    }

    let mut tx = db::begin_immediate(pool).await.map_err(db_err)?;
    let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT status, filename, analysis_json, novel_id FROM novel_imports WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let Some((status, filename, analysis_json, novel_id)) = row else {
        tx.rollback().await.map_err(db_err)?;
        return Err(not_found(ErrorCode::ImportNotFound, "导入任务不存在"));
    };
    if NovelImportStatus::from_db_or_default(&status) != NovelImportStatus::Ready
        || novel_id.is_some()
    {
        tx.rollback().await.map_err(db_err)?;
        return Err(AppError::new(
            ErrorCode::ImportConflict,
            "任务未就绪或已生成项目",
        ));
    }
    let raw = analysis_json.ok_or_else(|| AppError::internal("分析结果缺失"))?;
    let data: AnalysisData = serde_json::from_str(&raw)
        .map_err(|e| AppError::internal(format!("分析结果解析失败: {e}")))?;

    let mut truncated: Vec<&'static str> = Vec::new();
    let (title, was_truncated) = clip_field(
        req.title.as_deref().unwrap_or(data.title.as_str()).trim(),
        200,
    );
    if was_truncated {
        truncated.push("title");
    }
    let title = if title.trim().is_empty() {
        let stem = file_stem(&filename);
        if stem.trim().is_empty() {
            "未命名项目".to_string()
        } else {
            clip_field(&stem, 200).0
        }
    } else {
        title
    };
    let (background, t) = clip_optional(&data.background, 20000);
    if t {
        truncated.push("background");
    }
    let (world_view, t) = clip_optional(&data.world_view, 20000);
    if t {
        truncated.push("world_view");
    }
    let (summary, t) = clip_optional(&data.summary, 20000);
    if t {
        truncated.push("summary");
    }

    let novel_id = new_id();
    sqlx::query(
        "INSERT INTO novels(id, title, background, world_view, summary, status) \
         VALUES(?, ?, ?, ?, ?, 'active')",
    )
    .bind(&novel_id)
    .bind(&title)
    .bind(&background)
    .bind(&world_view)
    .bind(&summary)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let novel_body = [
        summary.as_deref(),
        background.as_deref(),
        world_view.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");
    search::index_entity(&mut tx, "novel", &novel_id, &novel_id, &title, &novel_body).await?;

    for (volume_index, volume) in data.volumes.iter().enumerate() {
        let (volume_title, t) = clip_field(&volume.title, 200);
        if t {
            truncated.push("volume.title");
        }
        let (volume_summary, t) = clip_optional(&volume.summary, 20000);
        if t {
            truncated.push("volume.summary");
        }
        let volume_id = new_id();
        sqlx::query(
            "INSERT INTO volumes(id, novel_id, volume_index, title, summary) VALUES(?, ?, ?, ?, ?)",
        )
        .bind(&volume_id)
        .bind(&novel_id)
        .bind((volume_index + 1) as i64)
        .bind(&volume_title)
        .bind(&volume_summary)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        search::index_entity(
            &mut tx,
            "volume",
            &volume_id,
            &novel_id,
            &volume_title,
            volume_summary.as_deref().unwrap_or(""),
        )
        .await?;

        for (chapter_index, chapter) in volume.chapters.iter().enumerate() {
            let (chapter_title, t) = clip_field(&chapter.title, 200);
            if t {
                truncated.push("chapter.title");
            }
            let (outline, t) = clip_field(&chapter.outline, 200000);
            if t {
                truncated.push("chapter.outline");
            }
            let chapter_id = new_id();
            sqlx::query(
                "INSERT INTO chapters(id, novel_id, volume_id, chapter_index, title, final_outline, status) \
                 VALUES(?, ?, ?, ?, ?, ?, 'outlined')",
            )
            .bind(&chapter_id)
            .bind(&novel_id)
            .bind(&volume_id)
            .bind((chapter_index + 1) as i64)
            .bind(&chapter_title)
            .bind(&outline)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            search::index_entity(
                &mut tx,
                "chapter",
                &chapter_id,
                &novel_id,
                &chapter_title,
                &outline,
            )
            .await?;
        }
    }

    let mut character_ids: HashMap<String, String> = HashMap::new();
    for character in &data.characters {
        let (raw_name, t) = clip_field(character.name.trim(), 50);
        if t {
            truncated.push("character.name");
        }
        if raw_name.trim().is_empty() {
            continue;
        }
        let name = import_name(&mut tx, "characters", &novel_id, &raw_name).await?;
        let (appearance, t) = clip_optional(&character.appearance, 20000);
        if t {
            truncated.push("character.appearance");
        }
        let (personality, t) = clip_optional(&character.personality, 20000);
        if t {
            truncated.push("character.personality");
        }
        let (hidden_personality, t) = clip_optional(&character.hidden_personality, 20000);
        if t {
            truncated.push("character.hidden_personality");
        }
        let (biography, t) = clip_optional(&character.biography, 20000);
        if t {
            truncated.push("character.biography");
        }
        let (current_goal, t) = clip_optional(&character.current_goal, 20000);
        if t {
            truncated.push("character.current_goal");
        }
        let (speaking_style, t) = clip_optional(&character.speaking_style, 20000);
        if t {
            truncated.push("character.speaking_style");
        }
        let role = crate::models::enums::RoleType::from_db_or_default(character.role_type.clone());
        let character_id = new_id();
        sqlx::query(
            "INSERT INTO characters(id, novel_id, name, role_type, appearance, personality, \
             hidden_personality, biography, current_goal, speaking_style) \
             VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&character_id)
        .bind(&novel_id)
        .bind(&name)
        .bind(crate::models::enums::RoleType::to_db(role))
        .bind(&appearance)
        .bind(&personality)
        .bind(&hidden_personality)
        .bind(&biography)
        .bind(&current_goal)
        .bind(&speaking_style)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        character_ids.insert(raw_name.to_lowercase(), character_id.clone());
        let body = [
            personality.as_deref(),
            biography.as_deref(),
            current_goal.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
        search::index_entity(&mut tx, "character", &character_id, &novel_id, &name, &body).await?;
    }

    for relation in &data.relations {
        let Some(a) = character_ids.get(&relation.a.trim().to_lowercase()) else {
            continue;
        };
        let Some(b) = character_ids.get(&relation.b.trim().to_lowercase()) else {
            continue;
        };
        let (a, b) = if a < b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        };
        if a == b {
            continue;
        }
        let (conflict_desc, t) = clip_optional(&relation.conflict_desc, 20000);
        if t {
            truncated.push("relation.conflict_desc");
        }
        sqlx::query(
            "INSERT OR REPLACE INTO character_relations(novel_id, char_a_id, char_b_id, affinity, trust, conflict_desc) \
             VALUES(?, ?, ?, ?, ?, ?)",
        )
        .bind(&novel_id)
        .bind(&a)
        .bind(&b)
        .bind(relation.affinity.clamp(-100, 100))
        .bind(relation.trust.clamp(0, 100))
        .bind(&conflict_desc)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }

    for item in &data.items {
        let (raw_name, t) = clip_field(item.name.trim(), 100);
        if t {
            truncated.push("item.name");
        }
        if raw_name.trim().is_empty() {
            continue;
        }
        let name = import_name(&mut tx, "items", &novel_id, &raw_name).await?;
        let (description, t) = clip_optional(&item.description, 20000);
        if t {
            truncated.push("item.description");
        }
        let (features, t) = clip_optional(&item.features, 20000);
        if t {
            truncated.push("item.features");
        }
        let owner_id = item
            .owner
            .as_deref()
            .map(str::trim)
            .map(str::to_lowercase)
            .and_then(|owner| character_ids.get(&owner).cloned());
        let item_id = new_id();
        sqlx::query(
            "INSERT INTO items(id, novel_id, name, features, description, owner_id, importance) \
             VALUES(?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&item_id)
        .bind(&novel_id)
        .bind(&name)
        .bind(&features)
        .bind(&description)
        .bind(&owner_id)
        .bind(item.importance.clamp(0, 5))
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }

    truncated.sort_unstable();
    truncated.dedup();
    let truncation = if truncated.is_empty() {
        None
    } else {
        Some(format!("部分字段超长已截断: {}", truncated.join(", ")))
    };
    let previous_message: Option<String> =
        sqlx::query_scalar("SELECT message FROM novel_imports WHERE id = ?")
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    let message = match (previous_message, truncation) {
        (Some(previous), Some(next)) if !previous.is_empty() => Some(format!("{previous}；{next}")),
        (Some(previous), Some(next)) => Some(format!("{previous}{next}")),
        (Some(previous), None) => Some(previous),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    };
    let claimed = sqlx::query(
        "UPDATE novel_imports SET novel_id = ?, message = ?, updated_at = strftime('%s','now') \
         WHERE id = ? AND novel_id IS NULL",
    )
    .bind(&novel_id)
    .bind(&message)
    .bind(job_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if claimed == 0 {
        tx.rollback().await.map_err(db_err)?;
        return Err(AppError::new(
            ErrorCode::ImportConflict,
            "项目已被创建，请勿重复提交",
        ));
    }
    tx.commit().await.map_err(db_err)?;

    novel::get(pool, &novel_id).await
}
