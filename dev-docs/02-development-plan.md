# StoryArchitect AI 多阶段开发计划（Master Plan）

> 基线：`00-design-v4.0.md` + `01-review-and-decisions.md`（D-xx 优先）
> 执行模式：**串行阶段交付**；每阶段结束跑验证 → 更新本计划 → `@oracle` 门禁 → 修复 → 提交 → 下一阶段。
> 目标产物：单文件本地优先小说大纲工具（Rust/Axum 后端 + React 前端，bundled SQLite）。

---

## 1. 阶段总览

| 阶段 | 名称 | 主要交付 | 依赖 | 验收门禁 |
|:---|:---|:---|:---|:---|
| P1 | 基础骨架与安全底座 | Cargo 项目、配置、错误、日志、DB/迁移、AppState、路由、Host/Origin、health、优雅关闭 | — | `cargo test`；health 200；HostGuard 单测 |
| P2 | 数据层与核心 CRUD | novels/characters/relations/items/volumes/chapters CRUD、分页校验、软删级联、回收站、FTS 同步、模板 seed | P1 | 端点集成测试；回收站往返；FTS 同步 |
| P3 | 契约生成与前端骨架 | ts-rs 导出、generated types、api client、Vite/React/Tailwind、布局路由、Dashboard/NovelDetail 骨架、store | P2 | `gen:types` + `typecheck`；黄金路径骨架 E2E |
| P4 | LLM 服务层 | reqwest client、Provider、能力探测、故障转移、结构化校验+修复、预算、模板渲染、usage 记录、mock 回放 | P1 | 结构化/故障转移/预算单测；mock 契约 |
| P5 | 沙盒引擎核心 | 事件枚举、SessionManager/hub、engine 主循环、Director 三模式+防重复、Actor、上下文/滚动摘要/重建、状态机、control/input、并发闸、崩溃恢复 | P2/P4 | 引擎 mock 测试；状态机全迁移；§8.8 场景 1/2/5/6/7 |
| P6 | SSE 与完整会话 API | SSE 补发/心跳/Lagged 自愈、sandbox.done、会话列表/快照/usage、waiting_input 超时、finalize 落大纲 | P5 | SSE 重放/Lagged；abort 即时取消；waiting 超时 |
| P7 | 导入导出 / 搜索 / 设置 / 成本 | export/import zip+manifest+ID 重映射、search、config GET/PATCH、usage 报表、备份、client-errors | P2/P4/P6 | 导入导出往返；版本拒绝；搜索；热更新 |
| P8 | 前端完整功能与发布 | 沙盒控制台三栏、设置、回收站、搜索、羁绊视图、大纲编辑、导出导入 UI、Playwright、CI 矩阵、release | P3/P6/P7 | E2E 全链路；性能预算；release 产物 |

> **关键路径**：P1→P2→P4→P5→P6；P3 可在 P2 后并行推进但视觉工作须由 @designer 主导；P7 依赖 P6 的会话事件；P8 最后整合。

---

## 2. 阶段门禁规则（Deepwork）

每阶段结束必须：
1. 运行该阶段验证命令，证据写入 `.slim/deepwork/story-architect-v4.md`；
2. 更新本计划阶段状态；
3. 发起 `@oracle` 门禁（携带：阶段目标、变更路径、验证证据、待评审决策/风险、已接受研究引用）；
4. 处理 material finding（一次有界修复），复验；
5. 独立可交付边界 → 创建聚焦 commit（用户明确要求时才 push）；
6. 进入下一阶段。

**Oracle 复审上限**：每门禁 1 次初审 + 最多 2 次复审。复审只针对未解决 material finding、修复引入的新风险，不重开已接受项。超限则记录残余风险并询问用户。

---

## 3. 代码组织约定

```
/                    Cargo.toml（单 crate）、build.rs、migrations/、frontend/、src/、tests/、scripts/
src/
  main.rs config.rs error.rs db.rs router.rs state.rs
  models/{mod,enums,novel,character,outline,item,sandbox,common}.rs
  services/{novel,character,outline,item,recycle,search,export,sandbox/*}.rs
  llm/{mod,client,providers/*,failover,structured,budget,prompts}.rs
  handlers/{novels,characters,relations,items,outline,sandbox,sse,config,usage,search,recycle,import_export,client_errors,health}.rs
  middleware/{host_guard,idempotency,request_log,body_limit}.rs
  jobs/{zombie,idempotency_cleanup,recycle_expiry,usage_rollup}.rs
```

- **命名**：数据库列 snake_case；Rust 结构 PascalCase；TS 类型由 ts-rs 生成，不手改。
- **错误**：服务层 `Result<T, AppError>`；handler 不 `unwrap`；clippy `unwrap_used=deny`。
- **事务**：跨表写单事务；`BEGIN IMMEDIATE` 统一封装 `db::begin_immediate(&pool)`。
- **时间**：Unix 秒 `i64`；金额 `f64` USD。
- **测试**：集成测试用 `tempfile` 真实库；LLM 一律 mock。

---

## 4. 阶段交付明细索引

- [P1 基础骨架与安全底座](phases/phase-01-foundation.md)
- [P2 数据层与核心 CRUD](phases/phase-02-data-crud.md)
- [P3 契约生成与前端骨架](phases/phase-03-contracts-frontend-shell.md)
- [P4 LLM 服务层](phases/phase-04-llm-layer.md)
- [P5 沙盒引擎核心](phases/phase-05-sandbox-engine.md)
- [P6 SSE 与完整会话 API](phases/phase-06-sse-session-api.md)
- [P7 导入导出/搜索/设置/成本](phases/phase-07-import-export-settings.md)
- [P8 前端完整功能与发布](phases/phase-08-frontend-release.md)

---

## 5. 全局 Definition of Done（每阶段适用）

- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 通过；
- 新增端点有集成测试（成功 + 主要错误路径）；
- 新增 DTO 有 `#[derive(TS)] #[ts(export)]`，`cargo test --test export_bindings` 可导出；
- 无密钥/Token 落日志；无 `unwrap()`（测试除外）；
- 迁移幂等（跑两遍无变化）；
- 文档更新（本计划 + 必要时 D 决策追加）。

---

## 6. 风险与假设

| 风险 | 应对 |
|:---|:---|
| 无真实 LLM 网络/密钥 | 全链路 mock provider + 录制回放；真实调用仅手动 smoke |
| 前端依赖安装耗时/离线 | 先 `npm install` 锁定；组件手写，减少依赖 |
| 单次会话上下文预算有限 | 严格分阶段，阶段结束即提交、可续 |
| sqlx bundled 首次编译慢 | 预留时间；不频繁 clean |
| macOS/Windows 签名不可本地验证 | CI 模板交付，标注"未执行" |

---

## 7. 当前状态

- [x] 设计文档归档
- [x] 深度审核与 D 决策
- [x] 多阶段计划文档
- [x] Oracle 计划审核与歧义消除（Gate 0: READY_TO_START_P1）
- [x] P1 基础骨架（Gate 1 PASS）
- [x] P2 数据层与核心 CRUD（Gate 2 PASS）
- [x] P4 LLM 服务层（Gate 3 修复完成）
- [x] P5 沙盒引擎（Gate 4 pending）
- [x] P6 SSE 与完整会话 API（Gate 5 PASS）
- [x] P7 导入导出/搜索/设置/成本（Gate 6 PASS）
- [x] P3/P8 前端与发布（Gate 7 PASS_WITH_FIXES，beta-ready）
- [x] 最终验证：cargo fmt/clippy/test 109 通过；frontend typecheck/build/test 通过；release 13MB + 运行时/浏览器冒烟通过
