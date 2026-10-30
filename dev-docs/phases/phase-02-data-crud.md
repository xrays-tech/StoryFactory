# P2 — 数据层与核心 CRUD

> 依赖：P1 | 门禁：@oracle
> 目标：完整的项目/角色/羁绊/物品/卷章 CRUD、软删级联、回收站、FTS 同步。

## 1. 范围

### 1.1 模型与 DTO
- `models/enums.rs`：`NovelStatus`、`RoleType`、`ChapterStatus` 等（D-06）。
- DTO（ts-rs）：`NovelDto`、`Create/UpdateNovelReq`、`CharacterDto`、`RelationDto`、`ItemDto`、`VolumeDto`、`ChapterDto`、`Page<T>`、`ApiError`。
- 校验：`validator`（标题长度、枚举、字符长度上限；§6.2 约束）。

### 1.2 服务与端点（§6.2）
- Novels：列表（分页/搜索/status）、创建（D-09 201+Location）、详情、PATCH、软删。
- Characters：列表/创建/详情/PATCH/软删；名称大小写不敏感唯一 → `NameConflict`。
- Relations：GET 矩阵、PUT 整体替换（D-10）。
- Items：列表/创建/详情/PATCH/软删。
- Volumes：列表/创建（index=max+1）/PATCH/软删（级联软删章）/move（up/down 交换）。
- Chapters：列表/创建（index=max+1）/详情/PATCH（含跨卷移动与重排）/软删。
- 分页 `Page<T> { items,total,page,page_size }`，page_size 上限 100。
- 所有写操作事务 + FTS 同步（D-05/D-22）。

### 1.3 回收站（§4.7）
- `GET /api/v1/recycle-bin?type=&page=`
- `POST /api/v1/recycle-bin/restore`（D-11 `deleted_batch` 批次、D-12 重命名）
- `DELETE /api/v1/recycle-bin/{type}/{id}`（硬删级联：novel/character/volume/chapter 规则；清 FTS）
- 自动过期 job 骨架（每日；P2 实现 job，P8 接 UI）。
- 软删级联同批次 UUID 写入 `deleted_batch`（D-11）；`deleted_at` 仅展示/过期。
- FTS `tokenize='trigram'`，中文检索按 D-22。

### 1.4 迁移与 seed
- `migrations/0001_init.sql` 已含 `deleted_batch`/`replay_hint`/`pending_directive`（P1）。
- `services/seed.rs`：四套 prompt 模板 seed v1（P4 使用，P2 先建表数据）。
- `jobs/`：幂等清理、回收站过期、僵尸扫描（骨架，P5 接入会话）、usage 聚合（P7）。

## 2. 测试
- 每端点：成功 + 404 + 409（唯一冲突）+ 422（校验）。
- 软删 → 列表消失 → 回收站可见 → 恢复 → 重现；恢复重命名。
- 级联软删批次恢复；单独删除子实体不被父恢复带回。
- 序号唯一：删除第 3 章后可重建第 3 章（部分唯一索引验证）。
- FTS：创建/更新/软删/恢复/永久清除后检索结果正确；`snippet` 高亮。
- 事务回滚：构造失败写入，断言无半态。

## 3. 验收
```bash
cargo test
# 手工：POST novel → POST volume → POST chapter → DELETE chapter → 回收站可见 → restore → 序号可复用
```
- 全部端点经 HostGuard；错误结构符合 §5.3。
- 契约测试（P3）前，DTO 已带 TS derive。

## 4. 决策引用
D-05、D-09、D-10、D-11、D-12、D-22。

## 5. 交付文件
`src/models/*`, `src/services/{novel,character,outline,item,recycle,search,seed}.rs`, `src/handlers/{novels,characters,relations,items,outline,recycle,search}.rs`, `src/jobs/*`, `tests/{crud,recycle,fts}.rs`.
