// src/models/dto.rs — CRUD DTO（§6.3, ADR-07: 所有 DTO 派 serde + TS + #[ts(export)]）
// Option 字段在 UpdateReq = 部分更新语义（缺=不变）。
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::models::enums::{ChapterStatus, MoveDirection, NovelStatus, RecycleEntity, RoleType};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NovelDto {
    pub id: String,
    pub title: String,
    pub background: Option<String>,
    pub world_view: Option<String>,
    pub summary: Option<String>,
    /// 文风档案：风格名（≤40 字）。
    pub style_name: Option<String>,
    /// 文风档案：参考范文（≤6000 字）。
    pub style_sample: Option<String>,
    /// 文风档案：额外写作要求（≤1000 字）。
    pub style_instruction: Option<String>,
    /// 文风档案：默认叙述人称（第一人称/第三人称）。
    pub pov: Option<String>,
    pub status: NovelStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateNovelReq {
    pub title: String,
    pub background: Option<String>,
    pub world_view: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateNovelReq {
    pub title: Option<String>,
    pub background: Option<String>,
    pub world_view: Option<String>,
    pub summary: Option<String>,
    /// None = 不改；Some("")/全空白 = 清空（写 NULL）；Some(text) = trim 后覆盖。
    pub style_name: Option<String>,
    /// None = 不改；Some("")/全空白 = 清空（写 NULL）；Some(text) = trim 后覆盖。
    pub style_sample: Option<String>,
    /// None = 不改；Some("")/全空白 = 清空（写 NULL）；Some(text) = trim 后覆盖。
    pub style_instruction: Option<String>,
    /// None = 不改；Some("")/全空白 = 清空（写 NULL）；仅接受 第一人称/第三人称。
    pub pov: Option<String>,
    pub status: Option<NovelStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CharacterDto {
    pub id: String,
    pub novel_id: String,
    pub name: String,
    pub role_type: RoleType,
    pub appearance: Option<String>,
    pub personality: Option<String>,
    pub hidden_personality: Option<String>,
    pub biography: Option<String>,
    pub current_goal: Option<String>,
    pub speaking_style: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateCharacterReq {
    pub name: String,
    pub role_type: Option<RoleType>,
    pub appearance: Option<String>,
    pub personality: Option<String>,
    pub hidden_personality: Option<String>,
    pub biography: Option<String>,
    pub current_goal: Option<String>,
    pub speaking_style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateCharacterReq {
    pub name: Option<String>,
    pub role_type: Option<RoleType>,
    pub appearance: Option<String>,
    pub personality: Option<String>,
    pub hidden_personality: Option<String>,
    pub biography: Option<String>,
    pub current_goal: Option<String>,
    pub speaking_style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RelationDto {
    pub char_a_id: String,
    pub char_b_id: String,
    pub affinity: i64,
    pub trust: i64,
    pub conflict_desc: Option<String>,
}

/// PUT /relations 请求（D-10: { relations: [...] }）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PutRelationsReq {
    pub relations: Vec<RelationInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RelationInput {
    pub char_a_id: String,
    pub char_b_id: String,
    pub affinity: Option<i64>,
    pub trust: Option<i64>,
    pub conflict_desc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ItemDto {
    pub id: String,
    pub novel_id: String,
    pub name: String,
    pub features: Option<String>,
    pub description: Option<String>,
    pub owner_id: Option<String>,
    pub importance: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateItemReq {
    pub name: String,
    pub features: Option<String>,
    pub description: Option<String>,
    pub owner_id: Option<String>,
    pub importance: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateItemReq {
    pub name: Option<String>,
    pub features: Option<String>,
    pub description: Option<String>,
    // JSON 显式 null 即置 NULL;缺即不变（JSON merge-patch 语义）
    pub owner_id: Option<Option<String>>,
    pub importance: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VolumeDto {
    pub id: String,
    pub novel_id: String,
    pub volume_index: i64,
    pub title: String,
    pub summary: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateVolumeReq {
    pub title: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateVolumeReq {
    pub title: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MoveReq {
    pub direction: MoveDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChapterDto {
    pub id: String,
    pub novel_id: String,
    pub volume_id: String,
    pub chapter_index: i64,
    pub title: String,
    pub scene_prompt: Option<String>,
    pub final_outline: Option<String>,
    #[serde(default)]
    pub beat_count: i64,
    pub status: ChapterStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateChapterReq {
    pub title: Option<String>,
    pub scene_prompt: Option<String>,
    pub status: Option<ChapterStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateChapterReq {
    pub title: Option<String>,
    pub scene_prompt: Option<String>,
    pub final_outline: Option<String>,
    pub status: Option<ChapterStatus>,
    // 跨卷移动（§6.2）: 提供任一即触发事务内重排校验
    pub volume_id: Option<String>,
    pub chapter_index: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecycleItemDto {
    pub entity_type: RecycleEntity,
    pub id: String,
    pub name: String,
    pub deleted_at: i64,
    pub novel_id: String,
    pub novel_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RestoreReq {
    pub entity_type: RecycleEntity,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchItemDto {
    pub entity_type: String, // 联合存储值 novel/volume/chapter/character
    pub entity_id: String,
    pub title: String,
    pub snippet: String,
}
