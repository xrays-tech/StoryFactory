// src/models/import.rs — txt 小说逆向分析 DTO（前端契约，ADR-07 ts-rs）
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NovelImportStatus {
    Queued,
    Analyzing,
    Ready,
    Failed,
    Cancelled,
}

impl NovelImportStatus {
    pub fn to_db(self) -> String {
        match self {
            NovelImportStatus::Queued => "queued".to_string(),
            NovelImportStatus::Analyzing => "analyzing".to_string(),
            NovelImportStatus::Ready => "ready".to_string(),
            NovelImportStatus::Failed => "failed".to_string(),
            NovelImportStatus::Cancelled => "cancelled".to_string(),
        }
    }

    pub fn from_db_or_default(raw: &str) -> NovelImportStatus {
        match raw {
            "analyzing" => NovelImportStatus::Analyzing,
            "ready" => NovelImportStatus::Ready,
            "failed" => NovelImportStatus::Failed,
            "cancelled" => NovelImportStatus::Cancelled,
            _ => NovelImportStatus::Queued,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            NovelImportStatus::Ready | NovelImportStatus::Failed | NovelImportStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NovelImportStage {
    Parsed,
    Chunking,
    AnalyzingChunks,
    Merging,
    Structuring,
    Ready,
    Failed,
}

impl NovelImportStage {
    pub fn to_db(self) -> String {
        match self {
            NovelImportStage::Parsed => "parsed".to_string(),
            NovelImportStage::Chunking => "chunking".to_string(),
            NovelImportStage::AnalyzingChunks => "analyzing_chunks".to_string(),
            NovelImportStage::Merging => "merging".to_string(),
            NovelImportStage::Structuring => "structuring".to_string(),
            NovelImportStage::Ready => "ready".to_string(),
            NovelImportStage::Failed => "failed".to_string(),
        }
    }

    pub fn from_db_or_default(raw: &str) -> NovelImportStage {
        match raw {
            "chunking" => NovelImportStage::Chunking,
            "analyzing_chunks" => NovelImportStage::AnalyzingChunks,
            "merging" => NovelImportStage::Merging,
            "structuring" => NovelImportStage::Structuring,
            "ready" => NovelImportStage::Ready,
            "failed" => NovelImportStage::Failed,
            _ => NovelImportStage::Parsed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NovelImportJob {
    pub id: String,
    pub filename: String,
    pub size_bytes: i64,
    pub status: NovelImportStatus,
    pub stage: NovelImportStage,
    pub progress: i64,
    pub chunks_total: i64,
    pub chunks_done: i64,
    pub message: Option<String>,
    pub error: Option<String>,
    pub novel_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviewVolume {
    pub title: String,
    pub summary: Option<String>,
    pub chapters: Vec<PreviewChapter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviewChapter {
    pub title: String,
    pub outline: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviewCharacter {
    pub name: String,
    pub role_type: String,
    pub appearance: Option<String>,
    pub personality: Option<String>,
    pub hidden_personality: Option<String>,
    pub biography: Option<String>,
    pub current_goal: Option<String>,
    pub speaking_style: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviewRelation {
    pub a: String,
    pub b: String,
    pub conflict_desc: Option<String>,
    // 模型可能漏字段（schema 里非必填时），缺省按 0 处理而不是让整块解析失败。
    #[serde(default)]
    pub affinity: i64,
    #[serde(default)]
    pub trust: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviewItem {
    pub name: String,
    pub description: Option<String>,
    pub features: Option<String>,
    pub owner: Option<String>,
    #[serde(default)]
    pub importance: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NovelImportPreview {
    pub job_id: String,
    pub title: String,
    pub world_view: Option<String>,
    pub background: Option<String>,
    pub summary: Option<String>,
    pub volumes: Vec<PreviewVolume>,
    pub characters: Vec<PreviewCharacter>,
    pub relations: Vec<PreviewRelation>,
    pub items: Vec<PreviewItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateNovelFromImportReq {
    #[serde(default)]
    pub title: Option<String>,
}

/// analysis_json 的内部形状（不导出 TS，前端只消费 NovelImportPreview）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AnalysisData {
    pub title: String,
    pub world_view: Option<String>,
    pub background: Option<String>,
    pub summary: Option<String>,
    pub volumes: Vec<PreviewVolume>,
    /// 分块阶段按原文顺序收集的「实际情节」条目，仅供内部审计；
    /// 对外 DTO（NovelImportPreview）不暴露该字段。
    #[serde(default)]
    pub beats: Vec<String>,
    pub characters: Vec<PreviewCharacter>,
    pub relations: Vec<PreviewRelation>,
    pub items: Vec<PreviewItem>,
}
