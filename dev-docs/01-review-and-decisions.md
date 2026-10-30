# StoryArchitect v4.0 设计深度审核与歧义裁决

> 审核对象：`dev-docs/00-design-v4.0.md`（v4.0，1923 行）
> 审核日期：2026-10-29
> 审核目的：判定文档是否可直接指导开发；把"可意会不可言传"的部分固化为可执行决策。
> 状态：**已审核，下列 D-xx 决策为开发基线；D 决策优先于设计文档中的模糊表述。**

---

## 1. 审核结论

**总体判定：可作为开发基线，但需以下裁决补齐 12 处实现级空白。** 设计文档在架构分层、数据模型、状态机、LLM 工程、安全与测试策略上已达到可开工水平；v3.0 的 34 项问题修复真实有效（抽查 S3/S4 部分唯一索引、S5 hub 模型、S6 摘要边界、S7 build.rs、S8 Host/Origin 均自洽）。

剩余问题不是"设计错误"，而是"实现前必须二选一/补定义"的空白，集中在：
- 构建期依赖（build.rs vs dev 编译、sqlx 查询宏 vs 运行时查询）；
- 跨层契约细节（DTO 与路由参数归属、枚举序列化、DELETE 语义）；
- 数据层机制（FTS5 行标识、级联软删批次、幂等响应快照上限）；
- 引擎运行时（并发许可生命周期、waiting_input 持久化、单步语义）。

以下 D-01 ~ D-24 为裁决结果，全部落入开发计划与代码规范。

---

## 2. 确认无问题的关键设计（不再重开）

| 主题 | 结论 |
|:---|:---|
| 单进程 Axum + Tokio，无 sidecar | 与"单文件分发"目标一致，采用 |
| sqlx + bundled SQLite（ADR-06） | 正确；`libsqlite3-sys` bundled 自带 FTS5/WAL |
| 落盘事务作为唯一背压点 + broadcast 扇出（§8.4） | 正确，SSE 用 seq 补发自愈 |
| `summary_until_seq` 摘要边界（§7.5/§8.5） | 闭环成立，采用 |
| 三层预算 attempt ⊂ step ⊂ session（§7.2） | 采用，全部以 `tokio::time::Instant` deadline 传递 |
| ts-rs 测试导出 + CI `git diff --exit-code`（ADR-07） | 采用 |
| HostGuard/OriginGuard + 0.0.0.0 启动令牌（§12.1） | 采用，安全左移进 P1 |
| 部分唯一索引 + 回收站级联（§4.1/§4.7） | 采用 |
| SSE 不压缩 + 心跳 + Last-Event-ID（§5.4/§6.4） | 采用 |

---

## 3. 裁决清单（Decision Log）

> 每条含：问题、候选、**裁决**、影响文件。开发时不得另行发挥。

### D-01 sqlx 查询方式：运行时 `query_as`，不用编译期宏
- **问题**：ADR-04 要求查询宏编译期校验，但宏需要构建期 `DATABASE_URL` + 已迁移数据库；本项目迁移在启动时执行，CI/新克隆无法在 `cargo build` 前提供库，导致构建不可复现。
- **裁决**：统一使用 `sqlx::query` / `sqlx::query_as::<_, Row>` 运行时查询，配合 `FromRow` 强类型行结构 + 集成测试覆盖 SQL。保留后续 `cargo sqlx prepare` 离线校验的升级路径，但 v4.0 不启用。ADR-04 记为实现修订。
- **影响**：`src/db.rs`、全部 `models/`、`services/`。

### D-02 build.rs 在开发期缺少 `frontend/dist` 的行为
- **问题**：build.rs 校验 `frontend/dist/index.html`，而开发期常先跑 `cargo run`，dist 不存在会直接编译失败，违背 §10.1 双终端工作流。
- **裁决**：build.rs 检测 `PROFILE`：
  - `debug`：dist 缺失时嵌入内置占位页（提示"前端未构建，请 npm run build"），`cargo:warning` 提示，不失败；
  - `release`：dist 缺失或 `index.html` 缺失 → `panic!` 构建失败。
  使用 `rust-embed` 的 `#[folder]` 指向 `frontend/dist`；占位页由 build.rs 写到 `OUT_DIR` 并作为备选嵌入。实现上采用"dist 存在则嵌入 dist，否则嵌入 `static/fallback`"的 `build.rs` 生成代码方案。
- **影响**：`build.rs`、`src/main.rs` 静态资源路由。

### D-03 版本号三元组
- **问题**：附录 C 写 `schema_version: 4`，但 §4.1 基线是 `0001_init.sql`，二者矛盾。
- **裁决**：`app_version = "4.0.0"`；`schema_version = 1`（= `_sqlx_migrations` 最新版本，运行时读取动态写入 manifest，见 D-25）；`format_version = 2`（附录 C 已声明当前为 2）。**导出时 schema_version 动态取库内值，不硬编码。**
- **影响**：`export_service`、manifest schema。

### D-04 幂等响应快照与 `replay_hint`
- **问题**：§4.1 注释提到 `>16KB 存摘要并标记 replay_hint`，但表无 `replay_hint` 列，也无行为定义。
- **裁决**：`idempotency_records` 增加 `replay_hint INTEGER NOT NULL DEFAULT 0`。作用域端点响应均 < 16KB，正常存完整 JSON；若响应 > 16KB：存 `{"replay_hint":true,"status_code":N}` 并置 `replay_hint=1`，重放返回该 JSON + `Idempotency-Replayed: true`，客户端据 `replay_hint` 重新 GET 资源。响应体硬上限 64KB。
- **影响**：`0001_init.sql`、`middleware/idempotency.rs`。

### D-05 FTS5 行标识与删除方式
- **问题**：`search_index` 的 `entity_id` 是 UNINDEXED 列，FTS5 不保证可高效按任意列删除；需要稳定 rowid 才能幂等重建。
- **裁决**：以实体 UUID（32 hex）前 16 hex 作为 `rowid`（`u64::from_str_radix`）。写入用 `INSERT OR REPLACE INTO search_index(rowid, title, body, entity_type, entity_id, novel_id)`；删除用 `DELETE FROM search_index WHERE rowid = ?`。UUID 前 64 bit 碰撞概率可忽略；写前用 `SELECT entity_id FROM search_index WHERE rowid=?` 断言无跨实体覆盖，命中不同 entity_id 则改用完整 64bit 哈希二次探测（实现为 `rowid_for(entity_id)` 冲突回退），并记录 warn。
- **影响**：`services/search.rs`、所有写事务。

### D-06 枚举序列化与命名
- **裁决**：所有领域枚举（`SessionStatus`、`DirectorMode`、`ControlAction`、`ErrorCode`、`Purpose`、`SandboxEventType`）`#[serde(rename_all = "snake_case")]`，ts-rs 生成字符串联合类型。`Purpose` 取值固定为 `actor_step|director|summary|repair|finalize`（文档 §4.1 注释为准，`rolling_summary` 归入 `summary`）。`SandboxEventType` 用 `snake_case`，`sandbox.done` 不落库、不进入 DB 枚举，仅 SSE 层常量。
- **影响**：`src/models/enums.rs`、`src/services/sandbox/events.rs`。

### D-07 路由参数与请求 DTO 归属
- **问题**：§6.3 `CreateSandboxSessionReq` 含 `novel_id`，但 §6.2 路由已含 `novel_id`/`chapter_id`。
- **裁决**：请求体**不含** `novel_id`/`chapter_id`，均从路径/上下文派生。定义两个请求结构共享核心字段：
  - `CreateChapterSessionReq { actor_ids, mode, scene_prompt, step_limit?, director_config?, user_directive?, autostart? }`
  - `CreateExplorationSessionReq { actor_ids, mode, scene_prompt, step_limit?, director_config?, user_directive?, autostart? }`（结构相同，语义不同，便于文档与校验差异）。
  - 服务端由 `cid` 查 `chapters` 得 `novel_id`；探索会话直接用路径 `novel_id`。
- **影响**：DTO、handlers、前端。

### D-08 配置优先级与热更新
- **裁决**：优先级（高→低）：`app_config` 表（仅白名单热键） > `APP_*` 环境变量 > `.env` > `config.toml` > 内置默认。启动时先读文件与环境，再读 DB 覆盖白名单键。`PATCH /api/v1/config` 仅接受白名单键（model、fallbacks、并发、预算、心跳、日志级别、超时、pricing、retention），写 DB + 原子替换运行时 `ArcSwap` 配置；非白名单返回 422。密钥不在此接口（仅 `APP_LLM_API_KEY` 环境变量或设置页专用密钥写入接口——v4.0 简化为环境变量 + 掩码只读展示）。
- **影响**：`config.rs`、`handlers/config.rs`、`state.rs`。

### D-09 DELETE 与创建响应语义
- **裁决**：`DELETE`（软删/永久清除）统一 `204 No Content`；`POST` 创建 `201 Created` + `Location`；`PATCH` 返回更新后实体 `200`；`PUT relations` 返回矩阵 `200`；`POST /control`、`/input` 返回 `202 Accepted` + 最新 `SessionSnapshot`（异步生效）；`POST /import` 返回 `201` + 导入报告。
- **影响**：全部 handlers、前端 api。

### D-10 羁绊矩阵 PUT 语义
- **裁决**：请求体 `{ relations: [{char_a_id, char_b_id, affinity, trust, conflict_desc?}] }`。服务端：校验两 ID 均属本 novel 且未软删、不得相同；归一化 `a<b`（交换并按需取 `-affinity`？**不取反**，affinity/trust 为对称字段，直接携带）；重复对以最后一条为准；事务内 `DELETE FROM character_relations WHERE novel_id=?` 后批量插入。空数组 = 清空。
- **影响**：`character_service`、relations handler、前端矩阵编辑器。

### D-11 软删级联与恢复批次（Oracle 修正，替代同秒时间戳方案）
- **问题**：Unix 秒粒度下同秒两次独立删除无法区分，父恢复会错误带回单独删除的子实体。
- **裁决**：5 张软删表（`novels/volumes/chapters/characters/items`）增加 `deleted_batch TEXT`（NULL=未删）。每次软删操作事务内生成一个 UUID，写入该操作及其级联影响的**全部行**。恢复父实体时仅恢复 `deleted_batch == 父实体 deleted_batch` 的子实体；单独删除的子实体批次不同，保持删除。`deleted_at` 仅用于展示与过期清理。恢复成功后将 `deleted_at`/`deleted_batch` 置 NULL。
- **影响**：`0001_init.sql`、`recycle_service`、所有软删 handler、回收站列表/过期 job。

### D-12 恢复/导入的重命名规则
- **裁决**：名称冲突 → 原名 + `（恢复）`；仍冲突则 `（恢复2）`、`（恢复3）`…（导入用 `（导入）` 同理）。卷/章序号冲突 → 取当前未删行 `max+1`，不重排其他行。单次操作最多尝试 100 次，超限返回 `NameConflict`。
- **影响**：`recycle_service`、`export_service`。

### D-13 并发许可生命周期（Oracle 修正）
- **问题**：§5.6 说创建与 recover 都要许可，但 `created` 态未运行会长期占坑；且 pause 后任务是否退出未定。
- **裁决**：许可在**引擎任务启动时同步获取**（autostart 的 create→start、手动 start、resume、recover、step），获取失败在响应前返回 429 + Retry-After；`created` 且 `autostart=false` 不占许可。**pause 时引擎任务在步边界退出并释放许可**；`resume` 重新 try_acquire 后启动新任务。许可随任务存活（`OwnedSemaphorePermit` 持有在引擎任务内，非长期 `SessionHandle` 字段）；`step` 执行单步后立即结束任务并释放。`SessionHandle` 只保留 `task` 与通信句柄。
- **影响**：`SessionManager`、`engine`、`handlers/sandbox`。

### D-14 waiting_input 的持久化与唤醒（Oracle 补强）
- **问题**：输入在内存丢失会导致 recover 后语义断裂；且引擎需在 waiting 时被唤醒；唤醒通道不可丢消息。
- **裁决**：`sandbox_sessions` 增加 `pending_directive TEXT`。`POST /input` 事务内：校验 `waiting_input`、写 `user_input_received` 事件、写 `pending_directive`、状态→`running`；提交后通过 `Notify`（`notify_one`，容量语义天然不丢最新状态）唤醒引擎，并发送 `watch` 快照。引擎下一步在**同一事务**内读取 `pending_directive` 作为 directive、落事件、清空 `pending_directive`；若事务失败则保留待重试。`waiting_input` 超时由引擎任务内 `tokio::select!`（300s）触发 finalize；进程崩溃则由启动恢复标记 interrupted，`pending_directive` 保留供 recover 使用。若引擎任务在 pause 后退出而状态为 waiting_input，`input` 会检测无活跃任务并同步启动续跑任务（同 recover 语义）。
- **影响**：schema、engine、handlers/input、SessionHandle。

### D-15 单步（step）语义实现
- **裁决**：`step` 仅允许 `paused` → 执行**恰好一步** → 回 `paused`。实现为引擎任务以 `RunMode::SingleStep` 启动，执行一步后不进入下一轮，写 `session_paused` 事件并结束任务/释放许可。`running` 下 step 返回 409。`created` 下 step 非法（先 start）。
- **影响**：engine 主循环、control handler。

### D-16 会话快照字段与 usage 聚合
- **裁决**：`SessionSnapshot.actor_names` 为 `Vec<ActorName { id, name, deleted: bool }>`（比元组更可读），ts-rs 生成对象数组。usage 从 `llm_usage` 按 session 聚合（成功+失败都计 calls），`by_purpose` 按 calls 降序。
- **影响**：DTO、session service、前端。

### D-17 LLM Provider 能力探测默认值
- **裁决**：`deepseek` 默认 `JsonObject`；`openai`/`openai_compat` 默认 `JsonSchema`；显式配置优先。探测失败按 `JsonObject`。`JsonObject` 模式把 Schema 的紧凑文本注入 system 末尾（≤800 token），并依赖本地 `jsonschema` 校验 + 修复回路。
- **影响**：`llm/providers/*`、`llm/structured.rs`。

### D-18 LLM 重试与预算边界
- **裁决**：attempt deadline = `min(now+attempt_timeout, step_deadline)`；重试只在 attempt 内按 1s/3s 退避，且每次重试前检查剩余预算 ≥ 2s，否则放弃。429 的 `Retry-After` 等待也受同一预算约束，超预算转故障转移。fallback 每跳独立 `LlmProvider` 实例与密钥。
- **影响**：`llm/client.rs`、`llm/failover.rs`。

### D-19 模板与 Prompt 安全
- **裁决**：`minijinja` 启用 `Sandboxed`（禁用 loader/文件系统），`auto_escape` 关闭（输出为纯文本 Prompt）。用户内容只作为变量值。模板 seed 在启动时若 `prompt_templates` 无对应 name 的 active 行则插入 v1；已存在不覆盖。
- **影响**：`llm/prompts.rs`、`migrations/seed.rs`。

### D-20 SSE 实现（Oracle 修正订阅顺序）
- **裁决**：SSE 路由**不经过** CompressionLayer（在 router 中单独 `route_layer` 排除）。事件流顺序必须是：**先 subscribe broadcast → 再查 DB `seq > after_seq` 补发 → 按 seq 去重 → 进入 live 流**；否则"查库完成"与"订阅建立"之间发布的事件会静默丢失（且不触发 Lagged）。`select!` 心跳（15s）/ 新事件 / 客户端断开。`Lagged` 时以 `last_sent_seq` 重新查 DB 补发。响应头 `Cache-Control: no-cache`、`X-Accel-Buffering: no`、`Connection: keep-alive`。`sandbox.done` 在会话终结事件后发送并结束流。live 流中 `seq <= last_sent_seq` 的事件直接丢弃（去重）。
- **影响**：`handlers/sse.rs`、`router.rs`。

### D-21 幂等中间件与事务边界（Oracle 修正为纯回放）
- **裁决**：幂等中间件对强制端点先查记录；业务 handler 在**同一事务**中写业务 + 幂等记录。中间件通过 `Extension<IdempotencyCtx>` 传递 key/scope/hash，handler 负责在事务内落记录（sqlx 事务不能跨中间件持有）。命中即**纯回放存储响应**（不查活会话、不 UPDATE 记录），附 `Idempotency-Replayed: true`；前端见该头即重新 GET 最新快照（会话存活/终结状态以 GET 为准）。同键异体 409；并发同键由主键冲突捕获后回查回放。只记录已完成响应（2xx 及会话语义终结 409）；TTL 24h，后台 10min 清理。
- **影响**：`middleware/idempotency.rs`、novels/sessions/import handlers、前端 client。

### D-22 全文检索同步范围与中文分词（Oracle 修正）
- **问题**：`unicode61` 不做 CJK 分词，中文正文整句成单 token，前缀匹配几乎失效。
- **裁决**：`search_index` 使用 `tokenize='trigram'`（bundled SQLite ≥3.34）。同步实体：novels（title/body=summary+background+world_view）、characters（name/body=personality+biography+current_goal）、volumes（title/summary）、chapters（title/scene_prompt+final_outline）。`items` 不进入 FTS。软删/恢复/永久清除同步。搜索：`|q| >= 3` 用 `MATCH '"' || q || '"'`（trigram 子串语义，无需 `*`）；`|q| == 2` 用 `LIKE '%'||q||'%'` 兜底（数据量本地可控）；`type` 过滤 `entity_type`；`bm25` 排序；`snippet/highlight` 保留。
- **影响**：`0001_init.sql`、`services/search.rs`、各写服务。

### D-23 导入导出
- **裁决**：zip crate 用 `zip`（deflate），`data.json` 单文件流式序列化（`serde_json::to_writer` 到临时文件再打包，避免内存峰值）。导出默认 `include_sessions=true`、`include_deleted=false`（查询参数可覆盖）。导入严格按附录 C 顺序：校验 checksum → 版本 → ID 重映射 → 唯一性预处理 → 单事务写 → 对账 counts。`llm_usage` 不导入。导入包内 events 的 `session_id` 重写。失败全回滚。
- **影响**：`export_service`、`handlers/import_export.rs`。

### D-24 前端实现边界（v4.0 首版）
- **裁决**：使用 Vite + React 18 + TS + Tailwind + Radix Primitives（按需）+ 手写轻量 shadcn 风格组件（Button/Dialog/Toast/Tabs/Input/Table）。不引入完整 shadcn CLI（避免网络/生成器耦合）。`@uiw/react-md-editor` 仅在大纲编辑页动态加载。SSE 用原生 `EventSource` 封装（§9.6）。状态用 Zustand + immer。路由用 React Router 6 数据路由。视觉与交互由 @designer 主导，禁止 fixer 自行改设计。
- **影响**：`frontend/` 全部。

### D-25 迁移器：`sqlx::migrate!`（Oracle 裁决）
- **问题**：设计 §4.2 自建 `schema_migrations`，实现文档又写"sqlx migrate 或自研"，二选一未定。
- **裁决**：使用 `sqlx::migrate!("./migrations")`（编译期嵌入、`_sqlx_migrations` 记录、每条迁移单事务、checksum 校验）。**删除自建 `schema_migrations` 表**。`schema_version` 读取 `SELECT MAX(version) FROM _sqlx_migrations`。启动时执行迁移；失败拒绝启动。破坏性变更仍遵守 §4.2 纪律。
- **影响**：`0001_init.sql`、`db.rs`、health、export manifest。

### D-26 0.0.0.0 模式下 SSE 的令牌传递
- **问题**：`EventSource` 不能自定义请求头，无法携带 `X-App-Token`。
- **裁决**：SSE 端点额外接受 `?token=` 查询参数；中间件在 `X-App-Token` 缺失时回退读取 query token（仅 SSE 路由）。token 不写日志/Referer 风险由本地场景接受；响应头 `Referrer-Policy: no-referrer`。
- **影响**：`middleware/host_guard.rs`、SSE 路由、前端 `sse.ts`。

### D-27 导入重映射完整性（Oracle 补充）
- **裁决**：除附录 C 列出的引用外，还必须重写：
  - `sandbox_events.payload` 内 JSON 的 `actor_id`、`next_actor_id`、`pair[0]`、`pair[1]`、`target_actor_id`；
  - `sandbox_sessions.director_config` 内 `[{actor_id, beat_hint}]` 的 `actor_id`；
  - `sandbox_sessions.actor_ids`（JSON 数组）。
  `chapters.sandbox_log` 为保留列，当前版本不写入、不导出、不重映射；导出以 `sandbox_events` 为准。
- **影响**：`export_service`、导入流程、附录 C 实现注释。

### D-28 开发期 CORS 开关
- **裁决**：仅当 `http.dev_cors=true`（默认 false）时挂载 CORS 层，且只放行 `http://localhost:5173`；OriginGuard 白名单同步加入该 origin。生产默认无 CORS 层。
- **影响**：`config`、`state`、`router`。

### D-29 启动令牌文件权限
- **裁决**：`APP_BIND` 非回环时生成 `app.token`，Unix 下 `chmod 0600`；令牌 32 hex。Windows 依赖 ACL 继承（文档标注）。
- **影响**：`main.rs`。

### D-32 前端 SSE 采用 fetch 流而非原生 EventSource（P8 实现裁决）
- **问题**：原生 `EventSource` 无法设置 `X-App-Token`/`Last-Event-ID` 请求头，也无法监听未知命名事件。
- **裁决**：前端 `SandboxEventStream` 用 fetch + ReadableStream 实现 EventSource 语义：`after_seq`、`Last-Event-ID`、指数退避 1s→30s、45s 假死重建、`sandbox.done` 关闭、未知事件兜底渲染。0.0.0.0 模式仍用 `?token=`（HostGuard 支持）。
- **影响**：`frontend/src/api/sse.ts`、`stores/sandboxStore.ts`。与 D-20 的服务端协议完全兼容。

### D-31 v4.0 全链统一 JsonObject + 本地 Schema 校验（P4/P5 入口）
- **裁决**：生产编排（`LlmService::complete_structured`）统一发送 `response_format=json_object` 并注入 Schema 摘要，以本地 `jsonschema` 校验为最终权威。Provider 的 `structured_mode` 与 `json_schema` 路径保留给直接调用/未来升级；`json_schema` 400 降级代码仅作用于直接调用路径。
- **理由**：DeepSeek 默认仅 json_object；统一路径避免链中混合 Provider 的能力差异，且本地校验已覆盖正确性。D-17 的“能力探测”在 v4.0 收敛为“默认 JsonObject + 可配置 Provider 直连模式”。
- **影响**：`llm/service.rs`、`llm/structured.rs`、P5 引擎。

### D-30 FTS rowid 映射表（P2 实现修订，替代 D-05 的纯 rowid 方案）
- **问题**：仅用 UUID 前缀 rowid 无法在删除时可靠定位（尤其碰撞回退后），且 FTS5 按 UNINDEXED 列删除不可靠。
- **裁决**：新增迁移 `0002_search_docs.sql`：`search_docs(entity_id TEXT PRIMARY KEY, rowid INTEGER NOT NULL UNIQUE)`。索引写入：先按 entity_id 查已有 rowid；无则用 UUID 前缀候选，冲突时哈希回退分配空位；同步 `search_docs` + `search_index`。删除按映射 rowid 删除两表。`novel` 级清理按 `search_index.novel_id` 收集 rowid 后删除。
- **影响**：`migrations/0002_search_docs.sql`、`services/search.rs`、所有 FTS 同步点。
- **备注**：新增 `ErrorCode::ItemNotFound`（404）用于物品资源；`RecycleEntity` 查询参数同时接受 `type` 与 `entity_type`。

---

## 4. 仍需在实现中验证的外部事实（风险，不阻塞）

| 项 | 状态 | 处理 |
|:---|:---|:---|
| DeepSeek 当前是否支持 `json_object` | 高置信支持 | 能力探测 + 失败降级；无网时 mock 测试 |
| `ts-rs` 10.x 导出目录行为 | 需实测 | 用 `TS_RS_EXPORT_DIR` 环境变量锁定；测试导出断言文件生成 |
| Radix/shadcn 依赖体积 | 中 | 只装实际使用的 `@radix-ui/react-*` |
| `sqlx` bundled 编译时间 | 中 | 首次编译较慢，可接受 |
| UPX 杀软误报 | 已知 | 发布双轨（压缩/非压缩），本地不强制 UPX |

---

## 5. 文档与设计不一致处的处理原则

1. 本文件 D-xx 与设计文档冲突时，**以 D-xx 为准**，并在实现处加注释 `// D-xx: ...`。
2. 设计文档未定义但实现必需的行为，按"最小惊讶 + 与既有约定一致"补齐，并回写本文件。
3. 任何新增 API 字段/端点必须先更新 `02-development-plan.md` 与 DTO，再实现。
4. 安全相关（Host/Origin/令牌/密钥/日志脱敏）不得为开发便利降级。

---

## 6. 审核发现的文档瑕疵（不影响开发，仅记录）

- §6.3 `CreateSandboxSessionReq.novel_id` 与路由重复 → D-07 已裁决。
- 附录 C `schema_version: 4` 与基线 0001 冲突 → D-03 已裁决。
- §4.1 幂等表注释提到 `replay_hint` 但无列 → D-04 已裁决。
- §8.3 状态图 ASCII 有重叠，但迁移表完整无歧义 → 以迁移表为准。
- §13.1 "4 会话运行 < 350MB" 与 "空闲 < 120MB" 需实测，作为发布门禁而非开发阻塞。
