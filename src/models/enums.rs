// src/models/enums.rs — 领域枚举（D-06: snake_case 序列化, ts-rs 字符串联合）
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NovelStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RoleType {
    Protagonist,
    Deuteragonist,
    Antagonist,
    Support,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChapterStatus {
    Draft,
    Outlined,
    Completed,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MoveDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RecycleEntity {
    Novel,
    Volume,
    Chapter,
    Character,
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EntityKind {
    Novel,
    Volume,
    Chapter,
    Character,
}

impl NovelStatus {
    pub fn to_db(s: NovelStatus) -> String {
        match s {
            NovelStatus::Active => "active".to_string(),
            NovelStatus::Archived => "archived".to_string(),
        }
    }
    pub fn from_db_or_default(raw: String) -> NovelStatus {
        match raw.as_str() {
            "archived" => NovelStatus::Archived,
            _ => NovelStatus::Active,
        }
    }
}

impl RoleType {
    pub fn to_db(s: RoleType) -> String {
        match s {
            RoleType::Protagonist => "protagonist".to_string(),
            RoleType::Deuteragonist => "deuteragonist".to_string(),
            RoleType::Antagonist => "antagonist".to_string(),
            RoleType::Support => "support".to_string(),
        }
    }
    pub fn from_db_or_default(raw: String) -> RoleType {
        match raw.as_str() {
            "protagonist" => RoleType::Protagonist,
            "deuteragonist" => RoleType::Deuteragonist,
            "antagonist" => RoleType::Antagonist,
            _ => RoleType::Support,
        }
    }
}

impl ChapterStatus {
    pub fn to_db(s: ChapterStatus) -> String {
        match s {
            ChapterStatus::Draft => "draft".to_string(),
            ChapterStatus::Outlined => "outlined".to_string(),
            ChapterStatus::Completed => "completed".to_string(),
            ChapterStatus::Archived => "archived".to_string(),
        }
    }
    pub fn from_db_or_default(raw: String) -> ChapterStatus {
        match raw.as_str() {
            "outlined" => ChapterStatus::Outlined,
            "completed" => ChapterStatus::Completed,
            "archived" => ChapterStatus::Archived,
            _ => ChapterStatus::Draft,
        }
    }
}

impl EntityKind {
    pub fn to_db(s: EntityKind) -> String {
        match s {
            EntityKind::Novel => "novel".to_string(),
            EntityKind::Volume => "volume".to_string(),
            EntityKind::Chapter => "chapter".to_string(),
            EntityKind::Character => "character".to_string(),
        }
    }
}

impl RecycleEntity {
    pub fn parse(raw: &str) -> Option<RecycleEntity> {
        match raw {
            "novel" => Some(RecycleEntity::Novel),
            "volume" => Some(RecycleEntity::Volume),
            "chapter" => Some(RecycleEntity::Chapter),
            "character" => Some(RecycleEntity::Character),
            "item" => Some(RecycleEntity::Item),
            _ => None,
        }
    }

    pub fn from_db_or_default(raw: String) -> RecycleEntity {
        match raw.as_str() {
            "volume" => RecycleEntity::Volume,
            "chapter" => RecycleEntity::Chapter,
            "character" => RecycleEntity::Character,
            "item" => RecycleEntity::Item,
            _ => RecycleEntity::Novel,
        }
    }
    pub fn to_db(s: RecycleEntity) -> String {
        match s {
            RecycleEntity::Novel => "novel".to_string(),
            RecycleEntity::Volume => "volume".to_string(),
            RecycleEntity::Chapter => "chapter".to_string(),
            RecycleEntity::Character => "character".to_string(),
            RecycleEntity::Item => "item".to_string(),
        }
    }
}
