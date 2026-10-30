// tests/export_bindings.rs — Rust DTO → TS 类型导出（ADR-07）
use story_architect::models::dto::{
    ChapterDto, CharacterDto, CreateChapterReq, CreateCharacterReq, CreateItemReq, CreateNovelReq,
    CreateVolumeReq, ItemDto, MoveReq, NovelDto, PutRelationsReq, RelationDto, RestoreReq,
    UpdateChapterReq, UpdateCharacterReq, UpdateItemReq, UpdateNovelReq, UpdateVolumeReq,
    VolumeDto,
};
use story_architect::models::sandbox::{
    ActorName, ControlReq, CreateSessionReq, DirectorMode, InputReq, SandboxEventType, ScriptStep,
    ScriptedPlan, SessionSnapshot, SessionStatus,
};
use ts_rs::TS;

#[test]
fn export_all_bindings() {
    let out = "frontend/src/types/generated";
    std::fs::create_dir_all(out).expect("create dir");
    NovelDto::export_all_to(out).expect("NovelDto");
    CreateNovelReq::export_all_to(out).expect("CreateNovelReq");
    UpdateNovelReq::export_all_to(out).expect("UpdateNovelReq");
    CharacterDto::export_all_to(out).expect("CharacterDto");
    CreateCharacterReq::export_all_to(out).expect("CreateCharacterReq");
    UpdateCharacterReq::export_all_to(out).expect("UpdateCharacterReq");
    RelationDto::export_all_to(out).expect("RelationDto");
    PutRelationsReq::export_all_to(out).expect("PutRelationsReq");
    ItemDto::export_all_to(out).expect("ItemDto");
    CreateItemReq::export_all_to(out).expect("CreateItemReq");
    UpdateItemReq::export_all_to(out).expect("UpdateItemReq");
    VolumeDto::export_all_to(out).expect("VolumeDto");
    CreateVolumeReq::export_all_to(out).expect("CreateVolumeReq");
    UpdateVolumeReq::export_all_to(out).expect("UpdateVolumeReq");
    MoveReq::export_all_to(out).expect("MoveReq");
    ChapterDto::export_all_to(out).expect("ChapterDto");
    CreateChapterReq::export_all_to(out).expect("CreateChapterReq");
    UpdateChapterReq::export_all_to(out).expect("UpdateChapterReq");
    RestoreReq::export_all_to(out).expect("RestoreReq");
    CreateSessionReq::export_all_to(out).expect("CreateSessionReq");
    ControlReq::export_all_to(out).expect("ControlReq");
    InputReq::export_all_to(out).expect("InputReq");
    SessionSnapshot::export_all_to(out).expect("SessionSnapshot");
    SessionStatus::export_all_to(out).expect("SessionStatus");
    DirectorMode::export_all_to(out).expect("DirectorMode");
    SandboxEventType::export_all_to(out).expect("SandboxEventType");
    ScriptedPlan::export_all_to(out).expect("ScriptedPlan");
    ScriptStep::export_all_to(out).expect("ScriptStep");
    ActorName::export_all_to(out).expect("ActorName");
}
