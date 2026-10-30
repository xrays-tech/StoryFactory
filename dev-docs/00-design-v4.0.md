# StoryArchitect AI 全栈系统设计方案 v4.0

> **文档版本**：v4.0（评审修复与深化扩展版）
> **基线文档**：v3.0（评审修复版，2026-10-28）
> **发布日期**：2026-10-28
> **状态**：可指导开发（Development-Ready）
> **阅读说明**：正文为 v4.0 全量设计，自洽完整、无需对照 v3.0 阅读；全文交叉引用已按 v4.0 章节号重排（修复：S1）；对 v3.0 的全部评审发现（34 项，含原文位置、根因、影响与修复去向）集中收录于**附录 A**，正文以「修复：Sx / Ax / Bx」角标标注对应关系。

---

## v3.0 → v4.0 变更总览

| 类别 | 内容 | 数量 |
|:---|:---|:---|
| 严重问题修复 | 交叉引用系统性错位、libsql/sqlx 混用矛盾、SQLite DDL 语法错误、软删除与唯一约束冲突、SSE 通道设计矛盾、摘要覆盖点缺失、build.rs 反模式、本地服务 Host/Origin 防护缺失 | S 级 × 8 |
| 重要问题修复 | 导演 Schema 与状态机脱节、启动/单步/恢复动作未定义、迁移编号冲突、缺失端点 × 4、FTS5 结构缺陷、导入体积冲突、超时体系冲突、DeepSeek 能力描述不实等 | A 级 × 15 |
| 一般问题修复 | 指标口径矛盾、tauri 残留、索引缺失、约束缺失、配置项散落等 | B 级 × 11 |
| 技术选型修正 | 数据库（libsql+sqlx 混用 → sqlx + bundled SQLite）；类型生成（build.rs 内嵌 cargo → ts-rs 测试导出 + CI 新鲜度断言）；跨平台编译（cross 全目标 → GitHub Actions 原生 runner 矩阵） | 3 项（ADR-06/07） |
| 深化扩展 | SandboxEvent 事件类型全集与 payload Schema；Actor/Director 输出 JSON Schema 完整定义；四套 Prompt 模板全文与变量表；会话状态机完整迁移表；三条关键时序图；单步上下文 token 布局图；并发场景矩阵 | 7 个方向 |
| 新增设计 | 通用幂等记录表与中间件；回收站（恢复/永久清除/自动过期）；本地服务 Host/Origin 双校验（防 DNS rebinding）；Prompt 注入威胁模型；导入导出格式规范（manifest.json + ID 重映射） | 5 处 |

---

# 1. 系统概述与设计目标

## 1.1 产品定位

StoryArchitect AI 是一个**本地优先、单文件分发**的小说大纲创作工具：用户配置角色、世界观与羁绊关系后，通过多智能体沙盒引擎让角色在场景下自主演进互动，最终沉淀为结构化章节大纲。产品的核心价值假设是「角色驱动的涌现剧情」比「人脑直写大纲」更能产生意外而合理的故事转折，而沙盒引擎的多智能体编排质量是这一假设成立与否的技术命门，因此本方案将引擎相关章节（§7、§8）作为设计重心。

## 1.2 设计目标与量化指标

| 目标 | 量化指标 | 测量口径 |
|:---|:---|:---|
| 零依赖分发 | 单一可执行文件 < 25MB（UPX 压缩后 < 15MB） | 产物体积 |
| 启动速度 | 冷启动（双击到可交互）< 1.5s，其中 Rust 进程启动 < 300ms | 人工 + 录屏脚本 |
| 资源占用 | 空闲内存 < 120MB；4 会话运行 < 350MB | CI 内存探针 |
| 沙盒单步延迟 | P95 < 4s，P99 < 8s（含一次 LLM RTT，DeepSeek 国内网络） | 录制回放 + 真实抽样 |
| 可靠性 | 进程崩溃后 100% 会话可标记恢复、可续跑，不残留僵尸任务 | 崩溃恢复演练 |
| 并发 | ≥ 4 个并行沙盒会话（LLM 配额受限时可配置降级为 2） | 并发压测 |
| 契约一致性 | Rust DTO 与 TS 类型零手工维护，CI 强制新鲜 | 契约测试 |
| 数据安全 | 密钥全程不明文落盘/落日志；误删 30 天内可恢复 | 安全用例 |

## 1.3 非目标（Out of Scope）

- 多人协作与云同步（v4.x 不考虑；数据访问层收敛于 Repository trait，为未来切换 Turso/libsql 远端保留替换空间——见 ADR-06）。
- 正文写作（只产出大纲，不产出成稿）。
- 移动端原生 App（Web UI 响应式即可）。
- 多语言 UI（v4.0 仅中文；文案集中管理便于后续 i18n）。

---

# 2. 技术栈选型

## 2.1 选型总表

| 层级 | 选型 | 最低版本 | 选型理由 / 备注 |
|:---|:---|:---|:---|
| 前端框架 | React | 18.3 | 并发渲染、成熟生态（React 19 已稳定，可平滑升级，不作为基线要求） |
| 样式 | Tailwind CSS + shadcn/ui + Radix Primitives | tw 3.4 | 原子化 CSS + 无障碍 headless 组件 |
| 状态管理 | Zustand + immer middleware | 4.5 | 轻量、TS 友好、支持跨 store 订阅 |
| 路由 | React Router（数据路由模式） | 6.26 | 路由级 loader 预取（v7 数据模式兼容） |
| 构建 | Vite | 5.4 | HMR 快、原生 ESM |
| Markdown | @uiw/react-md-editor + remark-gfm + DOMPurify | — | 编辑/预览一体；渲染前强制消毒（§12.2） |
| 虚拟滚动 | @tanstack/react-virtual | 3 | 事件流长列表 |
| 后端框架 | Axum | 0.7（0.8 兼容） | Tokio 生态、类型安全抽取器 |
| 异步运行时 | Tokio（rt-multi-thread + macros） | 1.40 | 事实标准 |
| **数据库** | **sqlx（sqlite flavor）+ libsqlite3-sys bundled** | **sqlx 0.8** | **修正（ADR-06）：放弃 libsql 混用；bundled 编译自带 FTS5 与 WAL 支持，零系统依赖** |
| LLM 客户端 | reqwest + reqwest-middleware + reqwest-retry | 0.12 / 0.4 | 连接池、超时、可定制重试策略 |
| 类型同步 | ts-rs（**测试导出模式**） | 10 | Rust serde 类型 → TS 接口；生成链路见 §6.1（ADR-07） |
| 静态嵌入 | rust-embed | 8.5 | 编译期嵌入 dist/；**仅做嵌入与产物存在性校验，不调用 cargo** |
| 配置 | config crate + dotenvy | 0.14 / 0.15 | 多层覆盖（§5.2） |
| 日志 | tracing + tracing-subscriber（JSON）+ tracing-appender | 0.3 | 结构化日志 + span 追踪 |
| 错误处理 | thiserror + 统一 AppError | — | 见 §5.3 |
| 并发集合 | dashmap | 6 | 会话注册表高并发读写 |
| 密钥内存保护 | secrecy | 0.10 | SecretString 零值化 + 日志脱敏 |
| 凭据加密 | aes-gcm + hkdf | — | 见 §12.3；优先系统 keyring |
| 输出校验 | jsonschema | 0.26 | LLM 结构化输出本地校验 |
| 模板引擎 | minijinja（沙箱特性） | 2 | Prompt 模板渲染，限制危险操作 |
| token 估算 | tiktoken-rs | 0.6 | cl100k 近似；按模型可配估算系数 |
| 校验 | validator + serde | 0.18 | 入参校验 |
| 测试 | cargo test + mockall + insta + proptest；Vitest + RTL + msw + Playwright | — | 见 §11 |

## 2.2 架构决策记录（ADR）

- **ADR-01 本地优先**：数据不出本机，网络仅用于 LLM API。理由：隐私 + 零后端运维成本。
- **ADR-02 SSE 而非 WebSocket**：演进流是单向的；SSE 原生重连（EventSource 自动携带 Last-Event-ID）+ 实现成本低。未来若加协作编辑再评估 WS。
- **ADR-03 ts-rs 而非手写 OpenAPI**：单机应用无第三方消费者，类型单向 Rust→TS 生成即满足契约一致性，成本远低于维护 OpenAPI 文档。
- **ADR-04 sqlx 编译期校验**：SQL 写在 .sql 迁移文件与查询宏中，编译期捕获列名/类型错误；运行时动态查询用 `sqlx::query` + 单元测试兜底。
- **ADR-05 沙盒引擎进程内运行**：不引入 sidecar 进程；会话隔离靠 Tokio task + DashMap 注册表，崩溃恢复靠会话状态持久化（§8.5）。单机场景下进程隔离的复杂度收益不抵成本。
- **ADR-06（修正）sqlx + bundled SQLite，放弃 libsql**：v3.0 同时引入 libsql 与 sqlx 并声称组合使用，但 sqlx 并无 libsql 驱动、libsql crate 与 sqlx API 互不兼容，二者只能选其一；且 v3.0 关于「libsql 同步驱动需 spawn_blocking 隔离」的描述与两库实际（均为异步）不符。v4.0 无云同步需求（非目标），SQLite 已满足 WAL/FTS5/零配置；数据访问收敛于 Repository trait，未来如需 Turso 同步仅替换实现层。**回退路径**：若 v5.x 引入云同步，新增 `libsql` crate 作为第二 Repository 实现，DTO 与 Service 层不变。
- **ADR-07（修正）类型生成走测试导出，build.rs 不调用 cargo**：cargo 在构建期间持有 target 目录构建锁，build.rs 内再执行 `cargo test` 存在死锁与递归构建风险，属反模式；ts-rs 官方路径是「`#[ts(export)]` + cargo test 触发导出」。v4.0 采用：`npm run gen:types` → `cargo test --test export_bindings` → 产物落 `frontend/src/types/generated/`；CI 执行后 `git diff --exit-code` 断言新鲜。build.rs 仅负责嵌入 dist 并校验 index.html 存在。
- **ADR-08（新增）本地服务 Host/Origin 双校验**：无鉴权的 127.0.0.1 HTTP 服务存在两类现实攻击面——DNS rebinding（攻击者域名解析到 127.0.0.1 绕过同源策略读取响应）与跨站请求（恶意页面发起的 no-cors 探测）。v4.0 以「Host 白名单 + 非 GET 请求 Origin 校验」作为最小代价防护（§12.1）；`0.0.0.0` 绑定模式额外启用启动令牌。

---
# 3. 系统架构

## 3.1 逻辑架构

```
┌────────────────────────────────────────────────────────────────┐
│                     用户交互层（Browser UI）                      │
│  React 18 + Tailwind + shadcn/ui │ Zustand stores │ React Router │
│  pages: Dashboard / NovelDetail(角色·羁绊·大纲) / Sandbox / Settings │
├────────────────────────────────────────────────────────────────┤
│                API 契约层（ts-rs 测试导出生成，CI 断言新鲜）          │
│   TS types ◄── serde DTO ◄── REST + SSE  /api/v1               │
├────────────────────────────────────────────────────────────────┤
│              业务编排层 Axum Handlers + Services                  │
│  Host/Origin 校验 → 校验(validator) → 幂等 → 事务 → 领域服务 → 统一响应 │
├───────────────┬──────────────────────┬─────────────────────────┤
│  领域服务      │  沙盒演进引擎（核心）    │  LLM 服务层              │
│  NovelService │  Director 调度器      │  Provider 链(故障转移)    │
│  CharService  │  Actor 执行器 ×N      │  能力探测(结构化输出)       │
│  OutlineService│ SessionManager+Hub  │  上下文压缩(RollingSum.)  │
│  ExportService│  事件落盘+SSE 广播     │  配额/成本记录             │
│  RecycleService│                     │                          │
├───────────────┴──────────────────────┴─────────────────────────┤
│      基础设施层：Config │ DbPool(sqlx/SQLite) │ Crypto │ Tracing   │
├────────────────────────────────────────────────────────────────┤
│  持久化：story_architect.db（WAL） + logs/ + config.toml + .env   │
└────────────────────────────────────────────────────────────────┘
                     │ reqwest（仅 LLM API 出网，host 白名单 §12.2）
                     ▼
               DeepSeek / OpenAI 兼容端点
```

## 3.2 运行时进程模型

单进程多线程（Tokio multi-thread，worker 数 = CPU 核数）：

- **主线程**：Axum serve，监听 `127.0.0.1:8080`（可配）。
- **数据库执行**：sqlx 的 SQLite 驱动内部通过专用阻塞线程执行 FFI 调用并在异步层桥接，**业务代码无需手动 `spawn_blocking`**（修正 v3.0「libsql 同步驱动需隔离」的过时描述，修复：S2）。
- **沙盒任务**：每个活跃会话一个 `tokio::task::JoinHandle`，挂入 SessionManager；取消语义见 §8.4（`select!` 即时取消）。
- **定时任务**：会话僵尸扫描（60s）、幂等记录过期清理（10min）、回收站自动清除（每日）、llm_usage 聚合（5min）、日志 flush。

## 3.3 部署形态

- **桌面形态**：双击单可执行文件，自动打开浏览器访问 `http://127.0.0.1:8080`（`webbrowser` crate）。数据目录遵循平台约定（Windows `%APPDATA%/StoryArchitect`；macOS `~/Library/Application Support/StoryArchitect`；Linux `~/.local/share/story-architect`），数据库文件 `story_architect.db` 与 `logs/`、`config.toml` 位于数据目录。
- **bind 策略**：默认仅 `127.0.0.1`；`APP_BIND=0.0.0.0` 时需显式确认，UI 常驻黄色警告条，且**强制启用启动令牌校验**（§12.1，修复：S8）。

---

# 4. 数据库设计（SQLite / sqlx）

> 修复：S2/S3/S4/A3/A5/A6/B5/B6/B10/B11。全部 DDL 为 v4.0 基线（`0001_init.sql`），语法经 SQLite 方言校验。

## 4.1 表结构（v4.0 基线）

**与 v3.0 的结构差异**（逐项理由见附录 A）：

1. 所有「软删除 + 唯一性」组合改为**部分唯一索引**（`CREATE UNIQUE INDEX ... WHERE deleted_at IS NULL`）——v3.0 写在 CREATE TABLE 内的 `UNIQUE (...) WHERE ...` 不是合法 SQLite 表约束；且 volumes/chapters 的全量 UNIQUE 会被软删行占位，导致「删了第 3 章就永远建不回第 3 章」。
2. `sandbox_sessions` 补 `summary_until_seq`（摘要覆盖边界，崩溃恢复的上下文重建依据）、`rolling_summary`、`template_versions`、`session_deadline`（会话级预算）——v3.0 拆在「迁移 0003」且两个不同变更都声称是 0003。
3. 新增 `idempotency_records`（通用幂等存储，支撑 novels/sessions/import 三类端点）。
4. FTS5 表的元数据列标记 `UNINDEXED`（仅存储不分词）。
5. `llm_usage` 补 `error_kind` 列与 `(session_id)` 索引；`prompt_templates` 补 `updated_at` 与「同 name 仅一条 active」部分唯一索引。
6. `characters` 名称唯一改为大小写不敏感（表达式索引），与 API 约定一致。

```sql
-- ============================================================
-- 0001_init.sql —— v4.0 完整基线
-- 说明：v3.0 为设计文档未发布，无存量库需要升级；
--       v4.0 直接以此文件为初始 schema，0002+ 留给后续演进。
-- ============================================================

-- ========== 小说项目 ==========
CREATE TABLE IF NOT EXISTS novels (
  id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  title       TEXT NOT NULL,
  background  TEXT,                          -- 故事背景
  world_view  TEXT,                          -- 世界观设定
  summary     TEXT,                          -- 全书简介
  status      TEXT NOT NULL DEFAULT 'active'
              CHECK (status IN ('active','archived')),
  created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  deleted_at  INTEGER                        -- 软删除时间戳，NULL=正常
);

-- ========== 卷 ==========
CREATE TABLE IF NOT EXISTS volumes (
  id            TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  novel_id      TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
  volume_index  INTEGER NOT NULL CHECK (volume_index >= 1),
  title         TEXT NOT NULL DEFAULT '',
  summary       TEXT,                        -- 本卷摘要（仅存一份，消除逐章冗余）
  created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  deleted_at    INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_volumes_index
  ON volumes(novel_id, volume_index) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_volumes_novel
  ON volumes(novel_id) WHERE deleted_at IS NULL;

-- ========== 章节大纲 ==========
-- novel_id 为受控冗余列（大纲树免 JOIN）；服务层事务内校验
-- volume_id 必属于同一 novel，二者一致性由应用层保证。
CREATE TABLE IF NOT EXISTS chapters (
  id            TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  novel_id      TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
  volume_id     TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
  chapter_index INTEGER NOT NULL CHECK (chapter_index >= 1),
  title         TEXT NOT NULL DEFAULT '',
  scene_prompt  TEXT,                        -- 场景设定
  sandbox_log   TEXT,                        -- 沙盒完整日志（JSON，导出用）
  final_outline TEXT,                        -- 定稿大纲（Markdown）
  status        TEXT NOT NULL DEFAULT 'draft'
                CHECK (status IN ('draft','outlined','completed','archived')),
  created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  deleted_at    INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_chapters_index
  ON chapters(volume_id, chapter_index) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_chapters_novel
  ON chapters(novel_id, chapter_index) WHERE deleted_at IS NULL;

-- ========== 角色 ==========
CREATE TABLE IF NOT EXISTS characters (
  id                 TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  novel_id           TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
  name               TEXT NOT NULL,
  role_type          TEXT NOT NULL DEFAULT 'support'
                     CHECK (role_type IN ('protagonist','deuteragonist','antagonist','support')),
  appearance         TEXT,
  personality        TEXT,
  hidden_personality TEXT,
  biography          TEXT,
  current_goal       TEXT,                   -- 导演决策输入之一
  speaking_style     TEXT,                   -- 沙盒 Prompt 用语言风格
  created_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  deleted_at         INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_characters_name
  ON characters(novel_id, lower(name)) WHERE deleted_at IS NULL;  -- 大小写不敏感（修复 B8），与 API 约定一致
CREATE INDEX IF NOT EXISTS idx_characters_novel
  ON characters(novel_id) WHERE deleted_at IS NULL;

-- ========== 角色羁绊矩阵 ==========
-- 行规范化：写入前强制 char_a_id < char_b_id（UPSERT 归一化）。
-- 不设 deleted_at：关系行跟随角色在「回收站清除」阶段统一清理（§4.7）。
CREATE TABLE IF NOT EXISTS character_relations (
  novel_id      TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
  char_a_id     TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  char_b_id     TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
  affinity      INTEGER NOT NULL DEFAULT 0 CHECK (affinity BETWEEN -100 AND 100),
  trust         INTEGER NOT NULL DEFAULT 0 CHECK (trust BETWEEN 0 AND 100),
  conflict_desc TEXT,
  updated_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  PRIMARY KEY (novel_id, char_a_id, char_b_id),
  CHECK (char_a_id < char_b_id)
);

-- ========== 物品 ==========
CREATE TABLE IF NOT EXISTS items (
  id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  novel_id    TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
  name        TEXT NOT NULL,
  features    TEXT,
  description TEXT,
  owner_id    TEXT REFERENCES characters(id) ON DELETE SET NULL,
  importance  INTEGER NOT NULL DEFAULT 0 CHECK (importance BETWEEN 0 AND 5),
  created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  deleted_at  INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_items_name
  ON items(novel_id, lower(name)) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_items_novel
  ON items(novel_id) WHERE deleted_at IS NULL;

-- ========== 沙盒会话 ==========
CREATE TABLE IF NOT EXISTS sandbox_sessions (
  id                TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  novel_id          TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
  chapter_id        TEXT REFERENCES chapters(id) ON DELETE SET NULL,
  scene_prompt      TEXT NOT NULL,
  actor_ids         TEXT NOT NULL,           -- JSON array；创建时校验均属本 novel 且未软删
  mode              TEXT NOT NULL DEFAULT 'director'
                    CHECK (mode IN ('roundrobin','director','scripted')),
  status            TEXT NOT NULL DEFAULT 'created'
                    CHECK (status IN ('created','running','paused','waiting_input',
                                      'finalizing','completed','failed','interrupted','aborted')),
  step_limit        INTEGER NOT NULL DEFAULT 20 CHECK (step_limit BETWEEN 1 AND 60),
  current_step      INTEGER NOT NULL DEFAULT 0,
  last_event_seq    INTEGER NOT NULL DEFAULT 0,
  summary_until_seq INTEGER NOT NULL DEFAULT 0,   -- 摘要覆盖边界：seq<=此值的内容已压缩进 rolling_summary
  rolling_summary   TEXT,                         -- 前情摘要（≤500 token）
  director_config   TEXT,                         -- scripted 模式行动脚本 JSON
  template_versions TEXT,                         -- 启动时所用模板版本快照 JSON
  session_deadline  INTEGER,                      -- 会话预算到期时刻（Unix 秒，start 时写入）
  summary_text      TEXT,                         -- finalize 产出的大纲结论
  error_code        TEXT,
  error_message     TEXT,
  idempotency_key   TEXT UNIQUE,
  created_at        INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at        INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  finished_at       INTEGER
);
CREATE INDEX IF NOT EXISTS idx_sessions_novel ON sandbox_sessions(novel_id, status);
CREATE INDEX IF NOT EXISTS idx_sessions_chapter ON sandbox_sessions(chapter_id);

-- ========== 沙盒事件日志 ==========
CREATE TABLE IF NOT EXISTS sandbox_events (
  seq          INTEGER NOT NULL,              -- 会话内单调递增，与 SSE id 一致
  session_id   TEXT NOT NULL REFERENCES sandbox_sessions(id) ON DELETE CASCADE,
  event_type   TEXT NOT NULL,                 -- 枚举见 §8.7
  payload      TEXT NOT NULL,                 -- JSON：SandboxEvent payload
  created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  PRIMARY KEY (session_id, seq)
);

-- ========== LLM 调用成本追踪 ==========
CREATE TABLE IF NOT EXISTS llm_usage (
  id                TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  session_id        TEXT REFERENCES sandbox_sessions(id) ON DELETE SET NULL,
  provider          TEXT NOT NULL,
  model             TEXT NOT NULL,
  purpose           TEXT NOT NULL,            -- actor_step / director / summary / repair / finalize
  prompt_tokens     INTEGER NOT NULL,
  completion_tokens INTEGER NOT NULL,
  cost_usd          REAL NOT NULL DEFAULT 0,
  latency_ms        INTEGER NOT NULL,
  success           INTEGER NOT NULL DEFAULT 1,
  error_kind        TEXT,                     -- 失败分类：network/timeout/rate_limited/invalid_output/auth/bad_request（修复 B9）
  created_at        INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_usage_session ON llm_usage(session_id);
CREATE INDEX IF NOT EXISTS idx_usage_created ON llm_usage(created_at);

-- ========== Prompt 模板 ==========
CREATE TABLE IF NOT EXISTS prompt_templates (
  id         TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  name       TEXT NOT NULL,                   -- actor_step / director_decide / rolling_summary / outline_finalize
  version    INTEGER NOT NULL,
  body       TEXT NOT NULL,                   -- minijinja 模板，支持 {{var}}
  is_active  INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  UNIQUE (name, version)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_prompt_active
  ON prompt_templates(name) WHERE is_active = 1;   -- 同名模板仅一条激活

-- ========== 通用幂等记录（v4.0 新增）==========
CREATE TABLE IF NOT EXISTS idempotency_records (
  key           TEXT NOT NULL,                -- 客户端 Idempotency-Key
  scope         TEXT NOT NULL,                -- 'novels' | 'sandbox_sessions' | 'import'
  request_hash  TEXT NOT NULL,                -- sha256(method+path+body)，同键异体检测
  status_code   INTEGER NOT NULL,
  response_body TEXT NOT NULL,                -- 成功响应快照（>16KB 时存摘要并标记 replay_hint）
  created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  PRIMARY KEY (key, scope)
);
CREATE INDEX IF NOT EXISTS idx_idem_created ON idempotency_records(created_at);

-- ========== 应用配置 ==========
CREATE TABLE IF NOT EXISTS app_config (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,                   -- is_secret=1 时为 AES-256-GCM 密文 Base64（§12.3）
  is_secret  INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- ========== 全文检索（FTS5）==========
-- 元数据列 UNINDEXED：仅存储、不参与分词（修复 A6：
-- 否则搜索 "chapter" 会命中所有章节行的 entity_type 字段）
CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
  title, body,
  entity_type UNINDEXED, entity_id UNINDEXED, novel_id UNINDEXED,
  tokenize = 'unicode61'
);

-- ========== 迁移表 ==========
CREATE TABLE IF NOT EXISTS schema_migrations (
  version     INTEGER PRIMARY KEY,
  applied_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  description TEXT NOT NULL
);
```

## 4.2 数据迁移与版本演进

- **迁移执行器**：`migrations/` 目录按文件名序（`0001_init.sql`、`0002_*.sql`…）；启动时 `Migrator` 读取 `schema_migrations`，按版本号顺序在**单事务**中执行未应用迁移，失败即启动失败并回滚该迁移（SQLite DDL 支持事务）。
- **基线策略**：v3.0 未发布、无存量库，v4.0 以 `0001_init.sql` 为完整基线（含 `rolling_summary`/`template_versions`/`summary_until_seq` 等列，修复 v3.0「迁移 0003 冲突且基线缺列」问题）。`PRAGMA user_version` 同步写入最新版本号，`PRAGMA table_info` 校验关键列存在性作为迁移自检。
- **破坏性变更纪律**：任何版本不得 DROP 删列；流程为「加列（默认填充）→ 双写（应用层）→ 发版 → 后续版本清理」。
- **跨版本数据迁移**：项目级导入导出走 `manifest.json` 声明的 `format_version`（见附录 C），与库内 schema 版本解耦。

## 4.3 数据备份

- **项目级导出**：设置页与 `GET /api/v1/novels/{id}/export`，流式生成 zip 包（manifest.json + data.json），格式见附录 C。
- **整库备份**：启动时若 `story_architect.db` 存在，自动复制为 `story_architect.db.bak`（保留最近 3 份轮转；复制前执行 `PRAGMA wal_checkpoint(TRUNCATE)` 保证备份包含全部 WAL 内容）。
- **恢复**：设置页「从备份导入」，整库替换前强制先备份当前库；替换后执行迁移自检。

## 4.4 全文检索

- **同步点（应用层，与写事务同体）**：novels/characters/volumes/chapters 的**创建、更新、软删除、恢复**四个动作都会在**同一事务**内同步 `search_index`（修复 B10：v3.0 只写了「写操作后同步」，遗漏恢复动作的索引重建）。永久清除事务中同步删除索引行。
- **索引内容**：title=名称/标题，body=简介/摘要/大纲文本；软删与恢复通过行插入/删除实现，无脏行。
- **查询**：`GET /api/v1/search?q=&type=&novel_id=&page=`；q 最小长度 2（配置 `fts_min_query_length`）；使用 `snippet()` 与 `highlight()` 返回高亮片段；前缀匹配 `"{q}*"` 提升中文体验。

## 4.5 连接与并发参数

```rust
// sqlx SqliteConnectOptions（每连接生效，由连接池工厂统一设置）
journal_mode = WAL;             // 读写并发
busy_timeout = 5000;            // 锁等待 5s，避免瞬时 SQLITE_BUSY
foreign_keys = ON;              // 每连接必开
synchronous = NORMAL;           // WAL 下安全且更快
// wal_autocheckpoint 默认 1000 页，无需调整
```

- 连接池 `max_connections = 4`（本地场景足够）；SQLite 单写者模型下，多连接写依靠 WAL + busy_timeout 排队，无需业务层额外写串行化。
- 所有多行写操作显式 `BEGIN IMMEDIATE`（sqlx：`sqlx::raw_sql("BEGIN IMMEDIATE")` 或 `Transaction` 默认行为），避免写锁升级失败。

## 4.6 索引策略汇总

| 表 | 索引 | 用途 |
|:---|:---|:---|
| volumes | uq(novel_id, volume_index) 部分唯一；idx(novel_id) 部分 | 防重复卷号；卷列表 |
| chapters | uq(volume_id, chapter_index) 部分唯一；idx(novel_id, chapter_index) 部分 | 防重复章号；大纲树加载 |
| characters | uq(novel_id, lower(name)) 部分唯一；idx(novel_id) 部分 | 角色名唯一（大小写不敏感）；列表 |
| items | uq(novel_id, lower(name)) 部分唯一；idx(novel_id) 部分 | 物品名唯一；列表 |
| character_relations | PK(novel_id, char_a_id, char_b_id) | 矩阵读写 |
| sandbox_sessions | idx(novel_id, status)；idx(chapter_id)；uq(idempotency_key) | 会话列表；章节关联；幂等 |
| sandbox_events | PK(session_id, seq) | 事件区间重放（seq > n 扫描） |
| llm_usage | idx(session_id)；idx(created_at) | 会话 usage 聚合；成本报表 |
| prompt_templates | uq(name, version)；uq(name) WHERE is_active=1 | 版本唯一；激活唯一 |
| idempotency_records | PK(key, scope)；idx(created_at) | 幂等命中；TTL 清理 |
| search_index | FTS5 内建 | 全文检索 |

> 全部业务索引均带 `WHERE deleted_at IS NULL` 部分条件（软删行不进索引，控制体积并保证唯一性语义正确）。

## 4.7 回收站与数据清除（v4.0 新增，修复 A12）

**软删除即入回收站**（novels/volumes/chapters/characters/items，`deleted_at` 非空即回收站内）：

| 操作 | 规则 |
|:---|:---|
| 软删除（DELETE 端点） | 事务内：置 `deleted_at` → 级联软删子实体（删卷级联软删其章；删小说级联全部）→ 同步删 `search_index` 行 |
| 列表 `GET /api/v1/recycle-bin?type=&page=` | 按 `deleted_at` 倒序，返回实体类型/名称/删除时间/所属小说 |
| 恢复 `POST /api/v1/recycle-bin/restore` | `{entity_type, ids[]}`；名称/序号冲突时自动重命名（名称追加「（恢复）」）或取当前 `max+1` 序号；事务内：清 `deleted_at` → 恢复被级联软删的子实体（仅当子实体未被单独删除）→ 重建 search_index 行 |
| 永久清除 `DELETE /api/v1/recycle-bin/{type}/{id}` | 硬删除 + 级联：novel→全子表硬删（含 sessions/events）；character→其 relations 行删除、items.owner_id 置 NULL；volume→其章硬删；chapter→`sandbox_sessions.chapter_id` 由 FK 置 NULL（历史会话保留，`sandbox_log` 不受影响）；同步清 search_index |
| 自动过期 | 后台任务每日一次清除 `deleted_at` 早于 `recycle_retention_days`（默认 30 天）的实体，执行前写 info 日志 |

> 历史沙盒会话的 `actor_ids` 引用已清除角色时**不改写**（会话是历史事实），控制台对缺失角色名显示 `(已删除角色)`。

---
# 5. 后端架构设计（Rust / Axum）

## 5.1 项目目录结构

```
story-architect/
├── Cargo.toml
├── build.rs                      # 仅 rust-embed 嵌入 + dist 存在性校验（不调用 cargo，ADR-07）
├── migrations/                   # 0001_init.sql …（sqlx migrate 兼容格式，编译期 include）
├── frontend/
│   ├── package.json
│   ├── vite.config.ts            # dev 代理 /api → 127.0.0.1:8080
│   └── src/
│       ├── components/           # common/ novel/ character/ outline/ sandbox/ settings/
│       ├── pages/                # Dashboard/ NovelDetail/ Sandbox/ Settings/
│       ├── stores/               # novel/ character/ outline/ sandbox/ ui
│       ├── api/                  # client.ts / sse.ts / endpoints.ts
│       ├── router.tsx
│       └── types/                # ts-rs 生成产物（提交入 git，CI 断言新鲜）
├── src/
│   ├── main.rs                   # 启动、优雅关闭、启动恢复例程
│   ├── config.rs                 # 多层配置加载与校验
│   ├── error.rs                  # AppError / ErrorCode → HTTP 映射
│   ├── db.rs                     # 连接池（ConnectOptions）、Migrator
│   ├── router.rs                 # 路由注册 + 分层中间件
│   ├── state.rs                  # AppState（config/db/llm/sessions/recycle 句柄）
│   ├── models/                   # sqlx 行结构 + serde DTO（#[derive(TS)] #[ts(export)]）
│   ├── services/
│   │   ├── novel_service.rs
│   │   ├── character_service.rs
│   │   ├── outline_service.rs    # 卷/章
│   │   ├── export_service.rs     # 附录 C 规范
│   │   ├── recycle_service.rs    # 回收站（§4.7）
│   │   └── sandbox/              # §8
│   │       ├── mod.rs            # SessionManager
│   │       ├── engine.rs         # 演进主循环
│   │       ├── director.rs       # 调度器（三模式）
│   │       ├── actor.rs          # 单角色执行
│   │       ├── context.rs        # 上下文组装/压缩/重建
│   │       ├── hub.rs            # 事件总线（落盘 + broadcast 扇出）
│   │       └── events.rs         # SandboxEvent 定义（§8.7）
│   ├── llm/                      # §7
│   │   ├── client.rs             # reqwest 客户端 + 中间件链
│   │   ├── providers/            # deepseek.rs / openai_compat.rs（能力探测）
│   │   ├── failover.rs           # Provider 链
│   │   ├── structured.rs         # Schema 校验 + 修复回路
│   │   ├── budget.rs             # 三层预算（§7.2）
│   │   └── prompts.rs            # 模板加载/渲染/版本快照
│   ├── handlers/                 # 薄控制器：抽取→校验→调服务→响应
│   ├── middleware/
│   │   ├── host_guard.rs         # Host 白名单 + Origin 校验（§12.1）
│   │   ├── idempotency.rs        # 通用幂等（§5.5）
│   │   ├── request_log.rs
│   │   └── body_limit.rs         # 分路由限额（默认 2MB / import 64MB）
│   └── jobs/                     # 僵尸扫描/幂等清理/回收站过期/usage 聚合
├── tests/                        # 集成测试（含 contract / export_bindings）
└── scripts/                      # release.sh / dev.sh / gen-types.sh
```

## 5.2 配置管理（多层覆盖）

优先级（高→低）：环境变量 `APP_*` > `.env` > `config.toml` > 内置默认。

```toml
# config.toml（数据目录，不含密钥）
bind = "127.0.0.1:8080"
data_dir = ""                        # 空 = 平台默认

[llm]
provider = "deepseek"
base_url = "https://api.deepseek.com/v1"
model = "deepseek-chat"
fallbacks = []                       # 例：["openai:gpt-4o-mini"]
connect_timeout_seconds = 5
attempt_timeout_seconds = 45         # 单次 LLM 尝试上限（§7.2）
max_retries = 2                      # 每次尝试链内的瞬时错误重试

[sandbox]
max_concurrent = 4
max_steps = 20                       # 会话默认步数上限（创建时可覆盖，1..=60）
step_timeout_seconds = 90            # 步预算
session_timeout_seconds = 1800       # 会话预算
history_max_tokens = 6000            # 滚动摘要触发阈值
context_hard_cap_tokens = 12000
rolling_summary_max_tokens = 500
director_repeat_limit = 3            # 同一角色连续行动硬上限（§8.2）
waiting_input_timeout_seconds = 300

[http]
body_limit_bytes = 2097152           # 默认 2MB
import_body_limit_bytes = 67108864   # 导入专用 64MB（修复 A7）
sse_heartbeat_seconds = 15

[recycle]
retention_days = 30

[cost]
monthly_budget_usd = 0.0             # 0 = 不限

[log]
level = "info"
trace_prompt = false                 # true 时记录 Prompt 全文（敏感，§12.5）
```

- `.env` / 环境变量仅存密钥：`APP_LLM_API_KEY`、`APP_LLM_FALLBACK_API_KEY_<N>`。
- 运行期可改项（模型名、并发数、预算等）经 `app_config` 表 + 设置页热更新，热更新立即生效项白名单见 §6.2；密钥仅设置页写入，读出永远掩码。
- **启动校验**（失败拒绝启动，给出可读中文错误）：模型名非空；attempt < step ≤ session 超时关系成立；并发数 ≤ 16；`recycle_retention_days ≥ 1`；bind 地址合法。

## 5.3 全局错误处理与错误码映射

```rust
pub enum ErrorCode {
    // 资源
    NovelNotFound, CharacterNotFound, VolumeNotFound, ChapterNotFound,
    SessionNotFound, RecycleItemNotFound,
    // 状态冲突
    SessionConflict,                 // 状态机非法迁移 / 单步语义非法
    IdempotencyConflict,             // 同幂等键不同请求体（v4.0 新增）
    NameConflict,                    // 唯一约束冲突（含恢复重命名失败）
    // 请求
    ValidationFailed, PayloadTooLarge, RateLimited,
    UnsupportedImportVersion,        // 导入格式版本不兼容（v4.0 新增）
    // LLM（LlmInvalidOutput 不直接面向端点，仅内部与诊断）
    LlmUnavailable, LlmRateLimited, LlmConfigMissing,
    // 系统
    ConfigInvalid, CryptoError, Internal,
}

pub struct AppError {
    pub code: ErrorCode,          // 机器可读，前端 switch
    pub message: String,          // 用户可读（中文）
    pub details: Option<serde_json::Value>,  // ValidationFailed 字段级明细
    pub request_id: Uuid,
    pub status: StatusCode,
}
// IntoResponse：{ "error": { "code", "message", "details?", "request_id" } }
```

**错误码 → HTTP → 前端处置映射表**（v4.0 新增，替代 v3.0 散落文字）：

| ErrorCode | HTTP | 典型触发 | 前端处置 |
|:---|:---|:---|:---|
| NovelNotFound / CharacterNotFound / VolumeNotFound / ChapterNotFound | 404 | 路径 ID 不存在或已软删 | Toast + 引导返回列表页 |
| SessionNotFound | 404 | 会话 ID 不存在 | Toast + 返回沙盒列表 |
| RecycleItemNotFound | 404 | 回收站条目不存在/已过期 | 刷新回收站列表 |
| SessionConflict | 409 | 状态机非法迁移（如 paused 下 pause） | 拉取最新 SessionSnapshot 静默对齐 |
| IdempotencyConflict | 409 | 同键不同体重放 | Toast「重复提交的请求内容不一致」 |
| NameConflict | 409 | 角色/物品/卷章名冲突 | 聚焦冲突字段 + 建议名 |
| ValidationFailed | 422 | 入参校验失败 | 字段级 inline 错误渲染 |
| PayloadTooLarge | 413 | 超分路由限额 | 提示内容上限 |
| RateLimited | 429 | 并发闸满 / 月预算超限 | 展示 Retry-After 倒计时，禁用提交 |
| UnsupportedImportVersion | 422 | 导入包 format_version 高于当前 | 引导升级应用版本 |
| LlmUnavailable | 503 | Provider 全链失败（同步调用场景） | 引导检查网络/密钥，提供重试 |
| LlmRateLimited | 429 | Provider 限流且无法继续 | 提示稍后重试 |
| LlmConfigMissing | 503 | 未配置 API Key 即发起会话 | 引导跳设置页 |
| ConfigInvalid | 500 | 启动期配置非法 | 启动横幅（不会出现在运行期 API） |
| CryptoError | 500 | 密钥解密失败 | 引导重置密钥（保留重置流程） |
| Internal | 500 | 未知异常兜底 | Toast + request_id 供反馈 |

规则：服务层返回 `Result<T, AppError>`；Handler 禁止 `unwrap`/`panic`（clippy `unwrap_used = "deny"`）；未知错误兜底 `Internal` 且日志带 span 上下文；`ValidationFailed.details` 结构 `[{field, message}]`。

## 5.4 中间件链（顺序即执行顺序）

```
TraceLayer(request_id 注入)
→ HostGuard(§12.1：Host 白名单)          ┐
→ OriginGuard(非 GET/HEAD 的 Origin 校验) ┘ 修复 S8
→ CompressionLayer(排除 /api/v1/sandbox-sessions/*/events 与 text/event-stream)
→ BodyLimit(默认 2MB；/api/v1/import 路由级 64MB)      # 修复 A7
→ CorsLayer(仅开发放行 http://localhost:5173)
→ RequestLog(method/path/status/耗时)
→ 路由分发
```

- **SSE 不压缩**（修复 A7）：CompressionLayer 对流式响应会缓冲破坏事件实时性，SSE 路由置于压缩层覆盖之外；SSE 响应头 `Cache-Control: no-cache`、`X-Accel-Buffering: no`。
- 请求日志 `info!` 含 request_id、user_agent、耗时 ms；>1s 自动 `warn!`；响应头统一 `X-Request-Id`。

## 5.5 事务边界与通用幂等（修复 A3/A5）

**事务边界**：

- Service 层显式 `tx = pool.begin()`；跨表写（建卷+章、导入、软删级联+FTS、事件落盘+状态推进、finalize 写 outline+events）单事务提交。
- 嵌套调用通过 `&mut Transaction` 参数传递，禁止服务间隐式开新连接。
- **同事务原则**：业务写、`search_index` 同步、`idempotency_records` 写入三者共事务，任一失败整体回滚。

**通用幂等中间件**（作用域：`POST /api/v1/novels`、`POST /api/v1/chapters/{cid}/sandbox-sessions`（强制）、`POST /api/v1/import`（可选））：

```
1. 读取 Idempotency-Key 头
   - 强制端点缺失 → 422 ValidationFailed
   - 可选端点缺失 → 服务端生成 UUID，响应头 Idempotency-Key 回传
2. 查 idempotency_records(key, scope)：
   - 命中且 request_hash 一致 → 直接回放存储响应，附加响应头
     Idempotency-Replayed: true（不执行业务）
   - 命中且 request_hash 不一致 → 409 IdempotencyConflict
   - 未命中 → 放行业务；业务成功后**同事务**写入记录与响应快照
3. 后台任务每 10min 清理 created_at 早于 24h 的记录（TTL）
```

- **并发同键竞争**（两个请求同键同时未命中）：SQLite 单写者 + `BEGIN IMMEDIATE` 使两事务串行化，后提交者命中唯一主键冲突 → 事务回滚 → 重查记录回放先入者响应，语义正确（测试用例见 §11.2）。
- 会话创建端点的幂等命中语义：命中且目标会话仍存活（created/running/paused/waiting_input/finalizing）→ 返回 200 + 现有会话；命中且已终结（completed/failed/aborted）→ 回放 409 SessionConflict 由前端确认重跑。终结态的 409 同样写入幂等记录（回放保持一致）。

## 5.6 并发治理（修复 A14）

- **全局并发闸**：`tokio::sync::Semaphore`（permits = `sandbox_max_concurrent`）；会话**创建与恢复（recover）**都需先获取许可，超限返回 429 + `Retry-After`（预估最早释放时间 = 最活跃会话的剩余步数 × 平均步长）。
- **即时取消**：每会话 `CancellationToken`；引擎在 LLM 调用等待期间通过 `tokio::select!` 监听令牌，收到取消立即丢弃 LLM future（reqwest future drop 即中止请求），**当前步不落盘**，状态直接迁移——修正 v3.0「每步结束后检查令牌」导致的 90s 级 abort 延迟：

```rust
tokio::select! {
    res = llm_call => handle(res),
    _ = cancel.cancelled() => return Err(Aborted.into()),  // 半步丢弃，无脏数据
}
```

- **步进与暂停竞争**：control 端点在 `BEGIN IMMEDIATE` 事务内校验状态合法性后写入目标状态；引擎每步开始前读取最新状态决定继续/暂停，状态推进与事件落盘同事务保证不丢事件。
- **僵尸扫描**：后台任务每 60s 扫描 `status IN ('running','finalizing')` 且 10 分钟无事件更新的会话 → 标记 `interrupted`（v3.0 遗漏 finalizing 态，已修复）；`paused/waiting_input` 超过 `session_deadline` 的会话同样标记。
- **waiting_input 超时**：达到 `waiting_input_timeout_seconds`（默认 300s）未收到用户输入 → 引擎按现有剧情进入 finalize（温和收束，不判失败）。

## 5.7 健康检查与优雅关闭

- `GET /api/healthz`：liveness（进程活着）。`GET /api/health`：readiness（DB 连通、PRAGMA 校验、schema 版本一致、LLM 配置存在——**不发真实 LLM 请求**，避免健康检查烧钱）。
- **优雅关闭**：Ctrl+C / SIGTERM → 停止接受新请求 → 通知运行中会话（CancellationToken + 10s 宽限）→ 宽限内完成的步骤正常落盘；超时未完成的步骤丢弃（半步不落盘）→ 未终结会话状态保持 running（由下次启动恢复例程标记 interrupted，§8.5）→ flush 日志 → 退出码 0。

---
# 6. API 契约设计

## 6.1 契约一致性机制（ADR-03 / ADR-07，修复 S7）

**生成链路（ts-rs 测试导出模式）**：

```
Rust DTO（#[derive(Serialize, Deserialize, TS)] #[ts(export)]）
   │  cargo test --test export_bindings   ← 唯一生成入口（ts-rs 官方路径）
   ▼
frontend/src/types/generated/*.ts         ← 提交入 git（可审查、可回滚）
   │
   ├── 开发：npm run gen:types（封装上述命令）
   └── CI：生成后 git diff --exit-code —— 有差异即失败（新鲜度断言）
```

- **build.rs 不参与类型生成**（修复 S7）：cargo 构建期间持有 target 目录锁，build.rs 内再触发 `cargo test` 会死锁/递归；build.rs 仅做 rust-embed 嵌入与 `frontend/dist/index.html` 存在性校验。
- 所有时间戳为 Unix 秒（INTEGER）；金额为 REAL（USD）；枚举两端均以字符串联合类型表示，**禁止魔法数字**。
- 前端 `api/endpoints.ts` 路径常量与 Rust 路由注册表同源维护，`tests/contract.rs` 断言路由表与端点文档一致。

## 6.2 REST 端点全表

| 方法 | 路径 | 说明 | 关键约束 |
|:---|:---|:---|:---|
| GET | /api/healthz · /api/health | 存活 / 就绪 | — |
| GET/POST | /api/v1/novels | 列表（分页+搜索）/ 创建 | POST 幂等可选 |
| GET/PATCH/DELETE | /api/v1/novels/{id} | 详情 / 更新 / 软删 | DELETE=软删入回收站 |
| GET/POST | /api/v1/novels/{id}/characters | 角色列表 / 创建 | 名称唯一（大小写不敏感） |
| GET/PATCH/DELETE | /api/v1/novels/{id}/characters/{cid} | 角色详情 / 更新 / 软删 | — |
| GET/PUT | /api/v1/novels/{id}/relations | 羁绊矩阵读 / 整体替换 | 服务端归一化 a<b；单事务 |
| GET/POST | /api/v1/novels/{id}/items | 物品列表 / 创建 | — |
| GET/PATCH/DELETE | /api/v1/novels/{id}/items/{iid} | 物品操作 | — |
| GET/POST | /api/v1/novels/{id}/volumes | 卷列表 / 创建 | volume_index 自动 = 未删卷 max+1 |
| GET/PATCH/DELETE | /api/v1/novels/{id}/volumes/{vid} | 卷操作 | 删卷级联软删其章 |
| PATCH | /api/v1/novels/{id}/volumes/{vid}/move | 卷移动（重排序） | `{direction: up/down}`，交换 volume_index，单事务 |
| GET/POST | /api/v1/novels/{id}/volumes/{vid}/chapters | 章列表 / 创建 | chapter_index 自动 max+1 |
| GET/PATCH/DELETE | /api/v1/novels/{id}/chapters/{cid} | 章详情 / 更新 / 软删 | PATCH 可含 `volume_id`+`chapter_index`（跨卷移动，事务内重排） |
| POST | /api/v1/chapters/{cid}/sandbox-sessions | 为章节发起沙盒会话 | **强制 Idempotency-Key**；429 超限 |
| POST | /api/v1/novels/{id}/sandbox-sessions | 发起与章节无关的探索会话 | 同上；chapter_id 省略 |
| GET | /api/v1/novels/{id}/sandbox-sessions | **按项目列会话**（修复 A4） | `?status=&page=`；含活跃/历史 |
| GET | /api/v1/sandbox-sessions/{sid} | 会话状态快照（含 usage 汇总） | — |
| GET | /api/v1/sandbox-sessions/{sid}/events | **SSE 事件流** | `?after_seq=n` 区间重放；Last-Event-ID 支持（§6.4） |
| POST | /api/v1/sandbox-sessions/{sid}/control | `{action}` | action ∈ start/pause/resume/step/abort/recover（§8.3 迁移表）；非法迁移 409 |
| POST | /api/v1/sandbox-sessions/{sid}/input | 用户注入指令 | waiting_input 态专用；`{text}` ≤2000 字符 |
| GET | /api/v1/novels/{id}/export | 导出项目 zip | 流式响应（附录 C） |
| POST | /api/v1/import | 导入项目 zip | **路由级 body 限额 64MB**（修复 A7）；事务内；幂等可选 |
| GET/PATCH | /api/v1/config | 读配置（密钥掩码）/ 热更新 | PATCH 白名单：model/concurrency/budget/heartbeat/log_level |
| GET | /api/v1/usage | LLM 成本报表 | `?month=YYYY-MM`，按日聚合 + purpose 分布 |
| GET | /api/v1/search | 全文检索 | q≥2 字符；type/novel_id/分页；高亮片段 |
| GET | /api/v1/recycle-bin | 回收站列表 | `?type=&page=` |
| POST | /api/v1/recycle-bin/restore | 批量恢复 | `{entity_type, ids[]}` |
| DELETE | /api/v1/recycle-bin/{type}/{id} | 永久清除 | 二次确认（前端）；不可恢复 |
| POST | /api/v1/client-errors | 前端异常上报（修复 A11） | 32KB 限额；写 tracing 日志（不建表防垃圾数据）；204 |

**通用约定**：

- 列表 `?page=1&page_size=20`（上限 100），返回 `{ items, total, page, page_size }`。
- 创建/更新请求体 `Content-Type: application/json`；服务端校验为最终裁决。
- 所有端点（除 /api/healthz）经 HostGuard/OriginGuard（§12.1）。

## 6.3 核心 DTO

```rust
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateSandboxSessionReq {
    pub novel_id: String,                 // 路径参数
    pub chapter_id: Option<String>,       // 修复 A5：可选（探索会话）；DB 列本就可空
    pub actor_ids: Vec<String>,           // 非空，1..=8，均属本 novel 且未软删
    pub mode: DirectorMode,               // "roundrobin" | "director" | "scripted"
    pub scene_prompt: String,             // 10..4000 字符
    pub step_limit: Option<u32>,          // 缺省取配置，1..=60
    pub director_config: Option<ScriptedPlan>,  // scripted 模式必填：[{actor_id, beat_hint}]
    pub user_directive: Option<String>,   // 用户全局指令（如"保持悬疑氛围"）≤500 字符
    pub autostart: Option<bool>,          // 默认 true：created→running（修复 A2）
}

#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionSnapshot {
    pub id: String,
    pub novel_id: String,
    pub chapter_id: Option<String>,
    pub status: SessionStatus,            // §8.3
    pub mode: DirectorMode,
    pub current_step: u32,
    pub step_limit: u32,
    pub actor_ids: Vec<String>,
    pub actor_names: Vec<(String, String)>,   // (id, name)，含已删角色标记
    pub last_event_seq: u64,
    pub can_recover: bool,                // status == interrupted
    pub usage: UsageSummary,
    pub error: Option<SessionError>,
    pub created_at: u64, pub updated_at: u64,
    pub finished_at: Option<u64>,
    pub session_deadline: Option<u64>,
}

#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UsageSummary {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost_usd: f64,
    pub calls: u32,                       // 成功 + 失败均计
    pub by_purpose: Vec<(String, u32)>,   // purpose → calls
}

#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ControlReq {
    pub action: ControlAction,            // "start"|"pause"|"resume"|"step"|"abort"|"recover"
}
```

## 6.4 SSE 线协议规范

```
GET /api/v1/sandbox-sessions/{sid}/events[?after_seq=n]
Accept: text/event-stream
```

```
event: actor_action          ← event = SandboxEvent.event_type（§8.7 枚举）
id: 42                       ← id = 落库 seq（单调递增），断线重连依据
data: {"step":7,"actor_id":"a1","action":{"type":"dialogue","content":"…"}}

: ping                       ← 每 15s 心跳注释行（可配置），防代理空闲断连

event: sandbox.done
data: {"summary_ref":"…"}    ← 会话终结信号，客户端收到后主动 close
```

**服务端行为**：

1. 连接建立 → 读取 `Last-Event-ID` 头或 `after_seq` 参数（取大者 n）→ `SELECT * FROM sandbox_events WHERE session_id=? AND seq>n ORDER BY seq` 补发 → 接入 live broadcast 流。
2. 会话已终结（completed/failed/aborted）→ 补发后发送 `sandbox.done` 并关闭连接。
3. 客户端（EventSource）自动重连并携带 Last-Event-ID；自定义重连策略见 §9.6。
4. 事件不重复、不丢失的依据：id=落库 seq，而「事件落盘与状态推进同事务」（§5.5），重放区间即数据库区间。

---
# 7. LLM 服务层

## 7.1 客户端封装

- 单一 `reqwest::Client` 全局复用（连接池），启用 `rustls-tls`、`gzip`、`http2`。
- 中间件链：`reqwest-middleware` + `reqwest-retry`（自定义 RetryableStrategy：**仅网络错误与 408/429/5xx** 可重试——修正 v3.0 的 409/425：409 对 LLM API 是非瞬态冲突，425 场景已被 429 覆盖）+ `AuthMiddleware`（注入 Bearer，日志脱敏）。
- 每次调用携带 `X-Request-Id`（与 tracing span 同 ID）；Provider 400 时响应体脱敏后记入日志。
- 模型抽象：

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn structured_mode(&self) -> StructuredMode;       // §7.4 能力探测
    async fn complete(&self, req: CompletionReq) -> Result<CompletionResp, LlmError>;
}

pub struct CompletionReq {
    pub messages: Vec<Message>,
    pub response_format: Option<ResponseFormat>,       // json_schema / json_object
    pub max_tokens: u32,
    pub temperature: f32,
    pub deadline: tokio::time::Instant,                // 预算制（§7.2）
    pub purpose: Purpose,                              // 计费归类
}
```

## 7.2 三层超时预算体系（修复 A9）

v3.0 的 `llm_timeout_seconds=60`、步超时 90s、重试 2 次三者叠加关系未定义，最坏情况 3 次调用 × 90s = 270s 远超步预算。v4.0 以**预算制**取代固定超时：

```
会话预算 sandbox.session_timeout_seconds = 1800
 └─ 步预算 sandbox.step_timeout_seconds = 90        （覆盖该步：导演调用 + 演员调用 + 修复回路）
     └─ 单次尝试 attempt_timeout = 45               （取 min(llm.attempt_timeout, 步预算剩余)）
         ├─ 连接超时 llm.connect_timeout_seconds = 5
         └─ 瞬时错误重试 ×2（退避 1s/3s，均计入步预算）
```

| 预算 | 耗尽后果 | 说明 |
|:---|:---|:---|
| 单次尝试 | 该次尝试计 `error_kind=timeout`，进入重试/故障转移 | deadline 以 `Instant` 传入 Provider |
| 步预算 | 本步标记 `actor_error`（不落任何半步数据） | actor_error 累计 ≥3 → 会话 failed（§8.2） |
| 会话预算 | 引擎进入 finalize，按现有剧情收束（**非 failed**） | `session_deadline` 在 start 时落库 |
| waiting_input 停留 | 超时 300s → finalize | 不判失败，剧情保持完整 |

- 导演调用使用步预算内的独立小预算（30s，剩余不足时取剩余）。
- 各 purpose 超时不再单列表格，统一由预算推导；配置项仅保留三个预算值（附录 B）。

## 7.3 Provider 故障转移链

```
请求 ──▶ Primary(DeepSeek) ──429/5xx/网络错──▶ Fallback-1(如 openai:gpt-4o-mini) ──▶ …
                                                                        │ 全链失败
                                                                        ▼
                                                    本地模板降级（最后手段，带 ⚠️ 降级标记）
```

- 链路来自 `llm.fallbacks` 配置；每跳独立密钥；**每跳均落 `llm_usage`（success=0 也记，含 error_kind）**。
- 限流感知：429 时解析 `Retry-After`（缺省 5s），等待后优先重试同 Provider 而非立即切换；等待时间计入所在预算。
- 不可重试错误：401（密钥错，立即失败并提示设置页）、400（请求错，脱敏记日志；若是结构化输出场景则进入修复回路 §7.4）。
- **模板降级输出**：带 `⚠️ 降级模式` 标记的占位大纲（场景概述 + 角色行动占位符），UI 显著提示、可一键重试——避免「降级输出看起来可用」的虚假安全感。

## 7.4 结构化输出与自动修复（修复 A13）

**Provider 能力探测**（修正 v3.0「DeepSeek/OpenAI 均支持 json_schema」的不实描述——DeepSeek 当前仅支持 `json_object`，`json_schema` 是 OpenAI 系能力）：

```rust
pub enum StructuredMode {
    JsonSchema { schema: serde_json::Value },   // response_format: {type:"json_schema"}（OpenAI 兼容）
    JsonObject { schema: serde_json::Value },   // response_format: {type:"json_object"}
                                                //   + Schema 摘要注入 system prompt 尾部（DeepSeek）
}
```

- 探测结果缓存于 Provider 实例（配置显式指定优先于自动探测；探测失败按 JsonObject 处理，向后兼容）。
- 响应侧统一走本地校验：`structured::extract_and_validate()` = 解析 JSON → `jsonschema` crate 校验 → 失败进入**修复回路**：把「原始输出 + 校验错误清单」回喂 LLM，要求仅返回修正后的 JSON（最多 2 轮，`purpose=repair` 计费）。
- 2 轮后仍失败：该步 `actor_error`（导演场景则降级轮转，§8.2），累计 3 次会话 failed。

**Actor 输出 JSON Schema（完整定义）**：

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "ActorStepOutput",
  "type": "object",
  "required": ["actor_action", "emotion_delta"],
  "additionalProperties": false,
  "properties": {
    "actor_action": {
      "type": "object", "required": ["type", "content"], "additionalProperties": false,
      "properties": {
        "type": { "enum": ["dialogue", "action", "inner_monologue", "scene_move"] },
        "content": { "type": "string", "minLength": 1, "maxLength": 800 }
      }
    },
    "emotion_delta": {
      "type": "object",
      "required": ["target_actor_id", "affinity_delta", "trust_delta"],
      "additionalProperties": false,
      "properties": {
        "target_actor_id": { "type": "string" },
        "affinity_delta": { "type": "integer", "minimum": -20, "maximum": 20 },
        "trust_delta":    { "type": "integer", "minimum": -20, "maximum": 20 }
      }
    },
    "director_note": { "type": "string", "maxLength": 200 }
  }
}
```

**Director 输出 JSON Schema（完整定义，修复 A1：补 `terminate` 与 `request_user_input`）**：

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "DirectorDecision",
  "type": "object",
  "required": ["next_actor_id", "reason"],
  "additionalProperties": false,
  "properties": {
    "next_actor_id": { "type": "string" },
    "reason": { "type": "string", "maxLength": 300 },
    "inject_scene_hint": { "type": "string", "maxLength": 200 },
    "terminate": { "type": "boolean" },
    "request_user_input": {
      "type": "object", "required": ["question"], "additionalProperties": false,
      "properties": {
        "question": { "type": "string", "maxLength": 300 },
        "suggestions": { "type": "array", "items": { "type": "string", "maxLength": 100 }, "maxItems": 4 }
      }
    }
  }
}
```

引擎按分支消费（Schema 层面 `next_actor_id` 恒必填，保证 LLM 易满足；分支裁决在引擎内）：

```
terminate == true                     → 会话进入 finalizing（reason 记入事件）
request_user_input 存在               → waiting_input（question/suggestions 推送前端）
否则                                   → next_actor_id 必须 ∈ actor_ids（非法 → 修复回路 → 仍非法降级轮转）
```

## 7.5 上下文窗口管理与滚动摘要（修复 S6）

**单步 Prompt token 布局图**（Actor 视角，预算合计 ≈ 10,700 ≤ 硬顶 12,000）：

```
┌─ system ──────────────────────────────────────────────── ≤1500 ┐
│  ① 引擎规则 + 输出 JSON 约定（模板固定段）                        │
│  ② 本角色卡（全量，超预算按 外观→隐藏性格→传记 顺序截断）           │
├─ user ─────────────────────────────────────────────────────────┤
│  ③ 场景设定 + 用户全局指令                        ≤800           │
│  ④ 情感矩阵快照（仅在场角色子矩阵）                ≤400           │
│  ⑤ 前情摘要 rolling_summary（覆盖 seq ≤ summary_until_seq）≤500  │
│  ⑥ 近期原文历史（events seq > summary_until_seq，旧→新）  ≤5500  │
│  ⑦ 本步导演指示（inject_scene_hint / beat_hint）    ≤200         │
├─ 输出预留 ─────────────────────────────────────────────────────┤
│  max_tokens ≥1500                                               │
└────────────────────────────────────────────────────────────────┘
```

- token 估算：tiktoken-rs cl100k 近似，按模型配置估算系数（中文场景经验值 ×1.1）。
- **压缩触发**（每步结束后检查）：`est(⑤+⑥) > history_max_tokens(6000)` 时：

```text
pick    = 自 summary_until_seq+1 起的最老连续事件，直至 est(剩余原文) ≤ 4000
summary = LLM(rolling_summary 模板, {previous_summary, steps: pick})   # 输出 ≤500 token
落盘事务：rolling_summary = summary（合并后仍 ≤500 token）
          summary_until_seq = pick 中最大 seq
          产生 sandbox_events: rolling_summary_applied 事件
```

- 原始事件永不删除（完整存于 `sandbox_events`，供回放/导出），压缩只影响 Prompt 组装。
- 硬顶双保险：`context_hard_cap_tokens=12000`（含全部组成）超限且无法再压缩 → 会话 failed（`error_code=CONTEXT_OVERFLOW`，不留半态）。
- **恢复时上下文重建**（§8.5）：`模板 + rolling_summary（代表 ≤summary_until_seq 的全部历史） + events(seq>summary_until_seq) 原文` —— v3.0 缺 `summary_until_seq` 边界，重建无法区分「已压缩/未压缩」，v4.0 以该列闭环（修复 S6）。

## 7.6 成本追踪与配额

- 每次调用落 `llm_usage`（provider/model/purpose/tokens/latency/cost/error_kind）。
- **成本单价**：`app_config` 键 `llm_pricing`（JSON：`{"deepseek/deepseek-chat": {"input": 0.27, "output": 1.10, "currency_per": 1e6}}`），设置页可编辑；缺失单价时 cost_usd 记 0 并在报表标注「单价缺失」。修正 v3.0「单价表内维护」未指明位置的问题（修复 B7）。
- 设置页展示：今日/本月花费、调用次数、按 purpose 分布、按日曲线；`GET /api/v1/usage` 数据源。
- 配额：`monthly_budget_usd`（默认 0=不限）超限时**新会话创建**返回 429 + 引导文案；进行中会话不中断（预算按创建时刻校验）。

## 7.7 Prompt 模板管理

- 模板存 `prompt_templates` 表（name, version, body, is_active；同名仅一条 active，§4.1 唯一索引保证）；渲染用 minijinja 沙箱特性，变量一律来自受控数据（角色卡/场景/历史），**用户内容只作为变量值注入，永不拼接进模板结构**。
- 内置四模板（首次启动 seed）；模板变更必须 version+1 且保留旧版；会话创建时快照 `template_versions`，历史会话可复现。
- 模板正文（minijinja 语法；`{%` 为指令、`{{` 为变量替换）：

**① actor_step（system 段）**：

```text
你是小说沙盒引擎中的角色扮演智能体，完全代入指定角色行动。

# 引擎规则
1. 只输出一个 JSON 对象，不输出任何其他文字。
2. JSON 结构（缺一不可）：
   {"actor_action": {"type": "dialogue|action|inner_monologue|scene_move", "content": "..."},
    "emotion_delta": {"target_actor_id": "...", "affinity_delta": -20..20, "trust_delta": -20..20},
    "director_note": "给导演的下一步建议（可选）"}
3. content ≤ 300 字；语言风格必须符合角色 speaking_style。
4. target_actor_id 必须是当前在场角色之一（可以是 自己，表示自我认知变化，此时增量一般为 0）。
5. 不替其他角色行动或说话；scene_move 仅用于移动/离场。
6. 遵守以下所有「设定」内容；设定之间冲突时，以本角色卡优先。

# 你扮演的角色
{{ self_card }}

# 场景
{{ scene }}

# 用户全局指令
{{ user_directive }}

# 情感快照（你的视角）
{{ emotion_matrix }}
```

**actor_step（user 段，按 §7.5 布局顺序拼装）**：

```text
【前情摘要】
{{ rolling_summary }}

【剧情记录】
{% for step in recent_history %}
第{{ step.no }}步 · {{ step.actor_name }}（{{ step.action_type }}）：{{ step.content }}
{% endfor %}

【本步指示】
{{ directive }}

现在轮到你了，只输出 JSON。
```

**② director_decide（system 段）**：

```text
你是沙盒导演，只做调度决策，绝不代替角色生成对话或行动。

# 决策规则
1. 只输出一个 JSON 对象：
   {"next_actor_id": "...", "reason": "...",
    "inject_scene_hint": "给该角色的暗示（可选）",
    "terminate": true/false（剧情自然收束时为 true）,
    "request_user_input": {"question": "...", "suggestions": [...]}（必须由用户裁决的分叉时使用，可选）}
2. 选择依据优先级：戏剧冲突潜力 > 角色未完成诉求（current_goal）> 情感张力变化 > 轮换公平。
3. 同一角色连续行动不超过 {{ repeat_limit }} 步；已连续 {{ consecutive_steps }} 步的角色尽量避免再选。
4. 场景目标已达成、冲突已解决或剧情自然到达收束点时 terminate=true。
5. 出现关键分叉（生死抉择、重大立场转变）且没有明确倾向时，用 request_user_input 交给用户。

# 场景
{{ scene }}

# 在场角色（含各自 current_goal）
{{ actor_cards_brief }}

# 情感矩阵
{{ emotion_matrix }}

# 已进行步数
{{ current_step }} / {{ step_limit }}
```

**director_decide（user 段）**：

```text
【前情摘要】
{{ rolling_summary }}

【近期剧情】
{% for step in recent_history %}
第{{ step.no }}步 · {{ step.actor_name }}：{{ step.content }}
{% endfor %}

决定下一个行动者，只输出 JSON。
```

**③ rolling_summary**：

```text
你是剧情压缩器。将给定剧情压缩为不超过 500 token 的前情摘要。

# 压缩规则
1. 保留：关键事件、人物关系变化、未解决的冲突、已埋伏笔。
2. 丢弃：寒暄、重复动作、纯环境描写、对剧情无影响的情绪表达。
3. 输出纯文本段落，按时间顺序，不使用列表与标题。
4. 与已有摘要合并：旧摘要在前，新内容在后，合并结果仍不超过 500 token。

# 已有摘要
{{ previous_summary }}

# 待压缩剧情
{% for step in steps %}
第{{ step.no }}步 · {{ step.actor_name }}（{{ step.action_type }}）：{{ step.content }}
情感变化：{{ step.emotion_note }}
{% endfor %}
```

**④ outline_finalize**：

```text
你是大纲撰写者。基于沙盒会话的事件流，为本章产出结构化大纲（Markdown）。

# 输出结构（严格遵守，使用以下二级标题）
## 章节标题          （一句话，≤20 字）
## 剧情梗概          （150-300 字）
## 关键场景          （2-5 个，每个包含：地点/人物/冲突/结果）
## 人物变化          （本章前后性格、关系、目标的变化）
## 伏笔与钩子        （埋设或回收的伏笔；结尾钩子）
## 衔接下一章        （1-2 句）

# 章节信息
{{ chapter_context }}

# 前情摘要
{{ rolling_summary }}

# 完整事件流（压缩呈现）
{% for step in steps %}
第{{ step.no }}步 · {{ step.actor_name }}（{{ step.action_type }}）：{{ step.content }}
{% endfor %}

# 用户全局指令
{{ user_directive }}
```

**变量表**：

| 变量 | 来源 | 预算/截断 |
|:---|:---|:---|
| `self_card` | characters 行（本人全量） | ≤800 token，外观→隐藏性格→传记顺序截断 |
| `scene` | session.scene_prompt | 入参校验 ≤4000 字符 |
| `user_directive` | session 创建参数 | ≤500 字符 |
| `emotion_matrix` | character_relations 在场子矩阵 | ≤400 token |
| `rolling_summary` | session.rolling_summary | ≤500 token |
| `recent_history` / `steps` | sandbox_events(seq>summary_until_seq) | ≤5500 token；finalize 用全量压缩呈现 |
| `directive` | inject_scene_hint / beat_hint / 「继续」 | ≤200 字符 |
| `actor_cards_brief` | 在场角色：名字+role_type+current_goal | 每角色 ≤60 token |
| `repeat_limit` / `consecutive_steps` / `current_step` / `step_limit` | 引擎状态 | — |
| `previous_summary` | 旧 rolling_summary | ≤500 token |

---
# 8. 多智能体沙盒演进引擎（核心章节）

## 8.1 引擎定位与总体流程

```
创建会话(幂等/并发闸) ──▶ start(autostart) ──▶ 组装初始上下文 ──▶ 主循环(每步):
    Director 选角 ──▶ Actor 执行(LLM+Schema校验/修复回路) ──▶ 应用情感增量
    ──▶ 落盘事件+状态(同事务) ──▶ hub 广播(SSE) ──▶ 检查终止/暂停/取消(select!)
    ──▶ 压缩检查(滚动摘要)
主循环退出 ──▶ finalize: 汇总大纲(LLM 或模板降级) ──▶ 写 chapter.final_outline(同事务)
           ──▶ status=completed ──▶ 推送 sandbox.done
```

**会话创建与运行时序图**：

```
 UI            API/Engine              Director           Actor/LLM          DB/Hub
 │ POST sessions │                        │                  │                 │
 │ (Idem-Key) ──▶│ 幂等检查+并发闸+校验      │                  │                 │
 │               │── INSERT session(created) ────────────────────────────────────▶│
 │ ◀─201 snapshot│                        │                  │                 │
 │ GET events ──▶│ SSE 订阅 ─────────────────────────────────────────────────────▶│
 │               │ start: created→running ─▶                 │                 │
 │               │   ┌─ 步循环 ────────────│──────────────────│─────────────────│
 │               │   │ select_next() ──────▶ (roundrobin/     │                 │
 │               │   │                      │  scripted 免LLM) │                 │
 │               │   │ director 模式: LLM ──┼─────────────────▶│ director_decide │
 │               │   │ next_actor_id ◀──────┼─────────────────│                 │
 │               │   │ actor_step: LLM ─────┼─────────────────▶│ 输出JSON+校验    │
 │               │   │ action+emotion ◀─────┼─────────────────│ (失败→修复回路)  │
 │               │   │── 事务: INSERT event + UPDATE session ──┼────────────────▶│
 │ ◀─event(id=n)─│◀── hub broadcast ────────┼─────────────────┼─────────────────│
 │               │   │ 令牌/终止/暂停检查 ────│                  │                 │
 │               │   └──────────────────────│──────────────────│─────────────────│
 │               │ finalize: outline LLM ──┼─────────────────▶│ outline_finalize │
 │               │── 事务: outline+events+completed ───────────┼────────────────▶│
 │ ◀─done ───────│◀── hub ──────────────────┼─────────────────┼─────────────────│
```

## 8.2 导演调度器 Director

三种模式（创建时选定，运行中不可切换）。**Director 只决定「谁行动」，内容一律由 Actor 生成**——每步输出可归因、可测试。

### 模式 A：roundrobin（轮转）

严格按 `actor_ids` 顺序轮转；无 LLM 开销。适用：快速跑通、测试、群像均衡戏份。

### 模式 B：director（导演驱动，默认推荐）

每步由**导演 LLM 调用**决定下一个行动者。输入输出与完整 JSON Schema 见 §7.4；决策依据优先级注入模板（戏剧冲突 > 未完成诉求 > 情感张力 > 轮换公平）。

**安全降级链**：导演输出非法（JSON 解析失败 / next_actor_id 不在 actor_ids / Schema 校验失败）→ 修复回路 1 轮 → 仍失败 → **降级为轮转**（记 warn 日志 + `director_fallback` 事件，会话不中断）。

**防重复选角**（v4.0 新增，修复 A15）：

- 软约束：模板注入「已连续 N 步的角色尽量避免再选」。
- 硬上限：`director_repeat_limit=3`——同一角色连续行动达上限时，引擎拒绝导演再次选择该角色，自动改选其余角色中「最久未行动」者，并发 `director_fallback` 事件说明原因。

**导演驱动的终止与等待**（修复 A1）：

- `terminate=true` → 会话进入 finalizing（reason 记入 `finalize_started` 事件）。
- `request_user_input` → 会话进入 `waiting_input`，question/suggestions 经事件推送前端；用户答复经 `/input` 端点注入（作为下一步的 directive）；300s 无响应自动 finalize（§5.6）。

### 模式 C：scripted（剧本驱动）

- 用户预定义行动序列 `[{actor_id, beat_hint}, ...]`；引擎严格按序执行，步数 = 序列长度（受 step_limit 约束，创建时校验）。
- beat_hint 作为该步 directive 注入 Actor Prompt，保证关键剧情落点。适合结构明确的章节。

**终止条件**（任一满足即 finalize）：

1. `current_step >= step_limit`；
2. director 模式：导演返回 `terminate=true`；
3. 用户主动 abort 或注入「结束」指令（结束指令走 finalizing 温和收束，abort 走 aborted 立即停止）；
4. actor_error 累计 ≥ 3（→ failed）；
5. 步预算内单步无法完成（计入 actor_error，同 4）；
6. 会话预算 `session_deadline` 到期（→ finalizing，温和收束）。

## 8.3 会话状态机与完整迁移表

```
              create(幂等/并发闸)
                     │
                     ▼
              ┌─ created ─┐
      start   │           │ abort
   (autostart)▼           ▼
        ┌── running ─────┐│
        │ ▲   │  │       ││
   resume│ │pause│  │director.        ││
        │ │   │  │request_user_input  ││
        │ │   ▼  ▼       ││        step(单步)
   ┌────│─┴ paused  waiting_input ────┐│
   │    │   │step│    │ input │超时   ││
   │    │   └───┘     ▼       │       ││
   │    │            running  ▼       ││
   │    └──────────────► finalizing ◀──┘│
   │ abort/失败             │成功        │
   ▼                        ▼           ▼
 aborted / failed       completed   (final_outline 已写)
        ▲
        │ 启动恢复例程 / 僵尸扫描（running/paused/waiting_input/finalizing → interrupted）
   interrupted ──recover──▶ running（上下文重建，§8.5）
```

**完整迁移表**（control 端点动作 = 触发；守卫不满足返回 409 SessionConflict）：

| from | 触发 | 守卫 | to | 副作用 |
|:---|:---|:---|:---|:---|
| created | start（autostart=true 或 control） | 并发闸许可 | running | 写 session_deadline；`session_started` 事件 |
| created | abort | — | aborted | 终结事件 |
| running | pause | — | paused | 状态落库；当前步完成后挂起 |
| running | director.request_user_input | — | waiting_input | 事件含 question/suggestions |
| running | 终止条件 1/2/3(结束)/6 | — | finalizing | `finalize_started` 事件 |
| running | actor_error≥3 / 上下文硬顶 | — | failed | error_code 落库；`session_failed` 事件 |
| running | abort | — | aborted | select! 即时取消（半步不落盘） |
| paused | resume | — | running | `session_resumed` 事件 |
| paused | step（单步执行） | — | paused | 执行恰好一步后回 paused（单步调试语义，修复 A2） |
| paused | abort | — | aborted | 同上 |
| waiting_input | input | 文本 ≤2000 字符 | running | `user_input_received` 事件，作为下一步 directive |
| waiting_input | 超时 300s | — | finalizing | 温和收束 |
| waiting_input | abort | — | aborted | — |
| finalizing | 大纲生成成功 | chapter_id 存在 | completed | 写 final_outline + `outline_ready` + `sandbox.done` |
| finalizing | 大纲生成失败（含模板降级成功则视为成功） | — | completed(降级标记) 或 failed | 降级时 summary_text 带 ⚠️ 标记 |
| running/paused/waiting_input/finalizing | 进程重启恢复例程 / 僵尸扫描 | — | interrupted | `session_interrupted` 事件；error_code=PROCESS_RESTART/ZOMBIE |
| interrupted | recover | 并发闸许可 | running | 上下文重建（§8.5）；`session_resumed(cause=recover)` |
| interrupted | abort / 用户重开新会话 | — | aborted / created(新) | — |

## 8.4 SessionManager 与事件总线（修复 S5）

v3.0 同段先写 `mpsc::Sender`（队列满阻塞=背压）又写 SSE 从 `broadcast::Receiver` 读取——mpsc 单消费者无法支撑多端同看，broadcast 无背压，「引擎阻塞在 send」会反压 LLM 步进与落盘。v4.0 改为 **落盘事务为唯一背压点 + broadcast 只做扇出**：

```rust
struct SessionManager {
    registry: DashMap<String, SessionHandle>,   // 仅活跃(created/running/paused/waiting_input/finalizing)
    sem: Arc<Semaphore>,                        // 全局并发闸（创建与 recover 共用）
}
struct SessionHandle {
    cancel: CancellationToken,
    task: JoinHandle<Result<()>>,
    hub: broadcast::Sender<SandboxEvent>,       // 每会话一个，容量 256
    snapshot: watch::Sender<SessionSnapshot>,   // 状态快照广播（控制台轮询兜底）
}
```

**事件双写的正确顺序**：

```
引擎产生事件
  ① 落盘事务（INSERT sandbox_events + UPDATE session.current_step/last_event_seq/…）
     ← 唯一背压点：SQLite 单写排队，天然限流
  ② hub.send(event)   ← broadcast 发送不阻塞、无订阅者时丢弃也无妨
  ③ 检查控制令牌 / 状态
```

- **多端订阅**：每个浏览器标签页一个 `broadcast::Receiver`；`RecvError::Lagged`（消费者落后超过 256 条）时，SSE 层**先补发 DB 区间再续流**（`last_sent_seq` → 当前 `last_event_seq`），利用「id=落库 seq」实现**无损自愈**——这正是落盘在前的回报。
- **注册表即活跃集**：进程重启后 registry 为空，库中活跃态会话由恢复例程统一改写（§8.5），杜绝内存与库不一致。
- **终结即关闭**：引擎发送 `sandbox.done` 后 drop hub，订阅者收到 `Closed`，SSE 连接优雅结束。

## 8.5 崩溃恢复与断线重放（修复 S6/A8）

**进程崩溃恢复**：

- 引擎每步结束即已把 `current_step/last_event_seq/rolling_summary/summary_until_seq` 落库（同事务），崩溃最多损失「正在进行的这一步」，无半步脏数据。
- **启动恢复例程**（main.rs 中，先于 HTTP serve）：

```text
UPDATE sandbox_sessions
   SET status='interrupted', error_code='PROCESS_RESTART', updated_at=now
 WHERE status IN ('running','paused','waiting_input','finalizing');
-- 每个受影响会话补发 session_interrupted 事件（seq 续接 last_event_seq+1）
```

- **recover 动作**（control 端点，修复 A8）：

```text
1. 校验 status == interrupted；获取并发闸许可（无则 429）
2. 上下文重建：
   base = {系统模板 + rolling_summary（代表 seq ≤ summary_until_seq 的全部历史）}
   tail = SELECT events WHERE seq > summary_until_seq          ← 原文进 Prompt
   重建后按 §7.5 布局组装；est 超硬顶则先执行一轮压缩再校验
3. created→running 语义复用：新 session_deadline = now + 会话预算
4. 状态 → running，session_resumed(cause=recover) 事件，SSE 续流（after_seq 从 last_event_seq 起）
```

**崩溃恢复时序图**：

```
 上次进程          重启后进程                DB                     UI
   │ ✖ 崩溃（第7步进行中） │                   │                      │
   │                     │ 启动恢复例程        │                      │
   │                     │── UPDATE → interrupted ─▶│                │
   │                     │   + session_interrupted 事件               │
   │                     │ serve ────────────────────────────────────▶│
   │                     │                     │   GET novel sessions │
   │                     │◀──────── 会话列表（interrupted，可续跑）──────│
   │                     │ POST control {recover}                      │
   │                     │ 闸许可+上下文重建(rolling_summary+tail)      │
   │                     │── status → running ─▶│                      │
   │                     │   从第 8 步继续主循环 │                      │
   │                     │◀── SSE 续流（after_seq=已见 seq）───────────│
```

**SSE 断线重连**：

```
 UI(EventSource)                    服务端
   │ ✖ 网络中断（已收至 id=41）          │
   │ （自动重连，携带 Last-Event-ID:41） │
   │ GET events ──────────────────────▶│
   │                       补发: SELECT events WHERE seq>41 ORDER BY seq
   │ ◀── 42,43,44…（补发）──────────────│
   │ ◀── live 流续接 ───────────────────│
   │ （若会话已终结：补发后 sandbox.done + 关闭）
```

- 心跳：每 15s（可配）发送 `: ping` 注释行，防代理/防火墙空闲断连。
- 前端重连策略（指数退避 1s→2s→…≤30s）见 §9.6。

## 8.6 情感矩阵应用规则

- Actor 输出的 `emotion_delta` 经限幅（Schema 层 -20..+20）后 UPSERT 写入 `character_relations`（事务内，写入前归一化 a<b；目标为「自己」时跳过写库仅记事件）。
- 写后夹紧值域（affinity ∈ [-100,100]，trust ∈ [0,100]），越界截断并记 debug 日志。
- 导演 Prompt 中只读快照，不写库——避免 LLM 幻觉覆盖用户手调值；用户在沙盒运行期手动调整羁绊，下一步的快照即生效（读写都在事务内，无脏读）。

## 8.7 SandboxEvent 事件类型全集（v4.0 新增）

`sandbox_events.event_type` / SSE `event:` 字段的**封闭枚举**（Rust 侧 `#[derive(TS)]` 枚举 + 前端联合类型同步生成；未知类型前端按 `unknown_event` 兜底渲染，保证前向兼容）：

| event_type | 触发时机 | payload 关键字段 | 前端渲染 |
|:---|:---|:---|:---|
| session_started | start 成功 | mode, actor_ids, step_limit, deadline | 控制台头部初始化 |
| session_paused / session_resumed | control | by(user/system), cause(recover 等), step | 状态徽章切换 |
| director_decision | 导演模式每步 | step, next_actor_id, reason, hint | 事件流「导演」气泡（可折叠） |
| director_fallback | 导演输出非法降级 | step, fallback_to(roundrobin), cause | 黄色提示条 |
| step_started | 每步开始 | step, actor_id | 步分组头 |
| actor_action | 演员行动成功 | step, actor_id, action{type,content}, emotion_delta, director_note | 事件流主体（按角色着色） |
| actor_error | 单步失败（含步预算耗尽） | step, actor_id, attempt, error_kind, message(截断200) | 红色条 + 重试态 |
| emotion_updated | 情感写入 | step, pair[a,b], affinity, trust | 右侧矩阵图联动 |
| rolling_summary_applied | 压缩完成 | until_seq, summary_tokens | 「前情已压缩」折叠条 |
| waiting_input | 导演请求输入 | step, question, suggestions[] | 底部输入框+建议按钮 |
| user_input_received | /input 注入 | step, text | 用户消息气泡 |
| finalize_started | 进入收束 | step, cause(step_limit/terminate/timeout/user) | 收束进度指示 |
| outline_ready | 大纲生成成功 | chapter_id, outline_markdown | 右栏大纲渲染（消毒后） |
| session_completed | 终结 | step, usage_summary | 完成横幅 + 成本汇总 |
| session_failed | 失败终结 | step, error_code, message | 错误横幅 + 重开入口 |
| session_aborted | 用户中止 | by, step | 中止横幅 |
| session_interrupted | 恢复例程/僵尸扫描标记 | cause(PROCESS_RESTART/ZOMBIE) | 「可续跑」提示 |
| sandbox.done | SSE 终结信号（不落库，仅线上） | — | 客户端 close EventSource |

> 落库事件与线上事件一一对应，唯 `sandbox.done` 仅在线上发送（其信息已由 session_completed/failed/aborted 落库承载）。

## 8.8 并发场景矩阵（v4.0 新增）

| # | 场景 | 竞态点 | 预期行为 | 验证方式 |
|:---|:---|:---|:---|:---|
| 1 | 同会话双开浏览器标签 | broadcast 多订阅者 | 各自独立接收；慢端 Lagged 后 DB 补发自愈 | 集成测试双订阅 + 人为限速 |
| 2 | 同 Idempotency-Key 并发创建会话 | 幂等记录写入竞争 | SQLite 单写串行；后入者主键冲突回滚→回放先入者响应，两客户端拿到同一 session | 集成测试并发 2×POST |
| 3 | 并发闸满时创建/恢复 | Semaphore | 429 + Retry-After（预估最早释放） | 单元测试闸计数 |
| 4 | recover 与新建会话并发 | 共用闸 | 二者公平竞争许可，互不挤占 | 集成测试 |
| 5 | pause 与步进竞争 | control vs 引擎 | control 事务改状态；引擎步首读取，最迟下一步生效；已开始的步完整落盘 | 集成测试时序注入 |
| 6 | abort 在 LLM 调用进行中 | select! | 即时取消（丢弃 LLM future），半步不落盘，状态→aborted | Mock Provider 慢响应 + abort |
| 7 | 两个 control 并发（pause+resume） | 状态机 | 事务串行化；后者按最新状态校验，非法者 409 | 集成测试并发 POST |
| 8 | 导入事务与沙盒落盘并发 | 单写排队 | busy_timeout 内排队等待；超时方重试一次后报错 | 集成测试 |
| 9 | 沙盒运行中软删其章节 | FK/引用 | 会话不受影响（chapter_id 置 NULL 由硬删触发；软删不触发）；finalize 时 chapter 缺失则仅存 summary_text | 单元测试 |
| 10 | 沙盒运行中软删其角色 | actor_ids 引用 | 会话继续（历史事实）；快照中角色名显示 `(已删除角色)`；导演模式降级轮转排除该角色 | 集成测试 |

---
# 9. 前端架构设计（React）

## 9.1 目录结构

```
frontend/src/
├── api/
│   ├── client.ts            # fetch 封装：超时/错误规范化/Idempotency-Key/X-App-Token
│   ├── sse.ts               # EventSource 封装：重连、Last-Event-ID、心跳、Lagged 自愈
│   └── endpoints.ts         # 与 §6.2 一一对应的路径常量与请求函数
├── stores/
│   ├── novelStore.ts        # 项目列表/当前项目
│   ├── characterStore.ts    # 角色+羁绊矩阵（编辑态与已保存态分离）
│   ├── outlineStore.ts      # 卷/章大纲树（v4.0 新增，修复 A10：§9.2 引用终于有名有实）
│   ├── sandboxStore.ts      # 会话状态、事件环形缓冲（最新 500 条）、连接状态
│   └── uiStore.ts           # 主题/侧边栏/toast/确认弹窗
├── pages/
│   ├── Dashboard.tsx        # 项目卡片、搜索、新建、回收站入口
│   ├── NovelDetail.tsx      # 标签页容器：概览/角色/羁绊/卷章大纲/物品/沙盒
│   ├── SandboxPage.tsx      # 会话列表 + 创建向导
│   ├── SandboxConsole.tsx   # 沙盒控制台（§9.4）
│   └── Settings.tsx         # LLM 配置/成本报表/备份导入导出/回收站/关于
├── components/
│   ├── common/              # ErrorBoundary/Toast/ConfirmDialog/EmptyState
│   ├── novel/ character/ outline/ sandbox/ settings/
├── router.tsx               # 数据路由：loader 预取，action 变更
└── types/                   # ts-rs 生成产物（唯一事实源，入 git）
```

## 9.2 状态管理与跨 store 通信

- **单一事实源**：服务端状态只经 api 层进入对应 store；组件不持有可写副本。`sandboxStore` 是唯一例外——SSE 事件流直接写入（高频，不经组件）。
- **跨 store 订阅**（Zustand `subscribeWithSelector`）：
  - `sandboxStore` 收到 `outline_ready` / `session_completed` → `outlineStore.fetchChapter(cid)` 刷新大纲树与编辑器（沙盒产出被动感知）。
  - `sandboxStore` 收到 `emotion_updated` → `characterStore` 局部更新矩阵图（不整树刷新）。
  - `characterStore` 保存成功 → `uiStore.toast` + 涉及该角色的关系行刷新。
- **乐观更新**：仅用于纯 UI 态（输入框、折叠）；数据提交一律等待服务端响应，避免与唯一事实源冲突。

## 9.3 路由设计

| 路由 | 页面 | loader 预取 |
|:---|:---|:---|
| `/` | Dashboard | novels 列表 |
| `/novel/:novelId` | 概览 | novel 详情 |
| `/novel/:novelId/characters` | 角色 | characters |
| `/novel/:novelId/relations` | 羁绊矩阵（力导向图/表格双视图） | relations + characters |
| `/novel/:novelId/outline` | 卷章大纲树 | volumes + chapters |
| `/novel/:novelId/items` | 物品 | items + characters |
| `/novel/:novelId/sandbox` | 沙盒会话列表+创建向导 | sessions（`GET /api/v1/novels/{id}/sandbox-sessions`） |
| `/sandbox/:sessionId` | 沙盒控制台 | session 快照 + `after_seq=0` 全量回放 |
| `/settings` | 设置 | config + usage |

- 离开 `/sandbox/:sessionId` 且会话活跃 → ConfirmDialog「后台继续运行 / 暂停 / 中止」（引擎在服务端运行，关页不杀会话）。
- 回收站、备份入口位于 `/settings`（独立 tab）。

## 9.4 沙盒控制台（三栏布局）

- **左：会话控制**——状态徽章（9 态色板）、步数进度（current/limit）、会话预算倒计时、start/pause/resume/step/abort/recover 按钮组（按状态机迁移表动态禁用）、用户注入指令输入框（waiting_input 态高亮）、成本实时显示（usage 轮询 30s + completed 事件对账）。
- **中：事件流**——虚拟滚动（@tanstack/react-virtual，500 条环形缓冲），按角色着色、按 step 分组，`director_decision` 折叠气泡，`rolling_summary_applied` 显示「前情已压缩」锚点；自动滚动可暂停。
- **右：大纲预览**——`outline_ready` 后渲染 `final_outline`（DOMPurify 消毒），一键「保存定稿」PATCH chapter.status='completed'；降级标记（⚠️）显著提示 + 重试按钮。

## 9.5 错误边界与反馈

- `ErrorBoundary`（class 组件）包裹各路由段：渲染异常 → 兜底页「重试 / 返回首页」+ 自动上报 `POST /api/v1/client-errors`（路由栈 + 组件栈，32KB 截断）。
- 统一 Toast：api 层解析 `error.code` → 中文文案映射（§5.3 映射表前端侧落地）；`SessionConflict/RateLimited/ValidationFailed/IdempotencyConflict` 有专属引导动作。
- 表单校验：前端先行（即时反馈），服务端校验为最终裁决（`ValidationFailed.details` 字段级 inline 渲染）。

## 9.6 SSE 客户端封装

```ts
class SandboxEventStream {
  constructor(sessionId: string, handlers: SandboxEventHandlers,
              opts: { onReconnect?: (attempt: number) => void })
  // 行为：
  // 1. EventSource 包装；断线指数退避重连 1s→2s→…≤30s，成功后清零
  // 2. 浏览器自动携带 Last-Event-ID（id= 落库 seq），服务端按区间补发
  // 3. ping 心跳驱动「连接中/已连接」指示；>45s 无任何事件视为假死，主动重建连接
  // 4. 未知 event 类型 → unknown_event 兜底渲染（前向兼容）
  // 5. sandbox.done → 主动 close 并回调 onDone
  // 6. recover 后重连使用 after_seq=本地最大 seq（双保险）
}
```

## 9.7 Markdown 与编辑器

- 编辑器：`@uiw/react-md-editor`（final_outline、卷摘要等长文本）。
- 渲染：remark-gfm + **DOMPurify 强制消毒**（§12.2），禁止内联 HTML 与 `javascript:` 协议链接。

---

# 10. 构建与发布

## 10.1 开发工作流

```
终端 1：cd frontend && npm run dev          # Vite :5173，代理 /api → 127.0.0.1:8080
终端 2：cargo watch -x "run -- --data-dir ./dev-data"   # 隔离开发数据目录
类型生成：npm run gen:types                  # = cargo test --test export_bindings（§6.1）
提交前：npm run typecheck && cargo clippy -- -D warnings（pre-commit 钩子）
```

## 10.2 生产构建流水线

```
1. npm ci && npm run gen:types && npm run build   # 类型新鲜 + Vite 产物 → frontend/dist
2. cargo test --test export_bindings && git diff --exit-code -- frontend/src/types
                                                  # 类型与 git 一致性断言（ADR-07）
3. cargo build --release                          # rust-embed 嵌入 dist
                                                   # build.rs 校验 dist/index.html 存在
4. strip + UPX（可选，目标 <15MB；见 §16 UPX 误报风险）
5. 产物：story-architect[.exe] 单文件 + SHA-256 校验文件
```

## 10.3 跨平台编译（修复 B4）

v3.0 声称用 `cross` 构建全部 5 目标，但 cross **不支持** `x86_64-pc-windows-msvc`（需 Windows SDK，容器内不可用）。v4.0 改为 GitHub Actions **原生 runner 矩阵**：

| 目标 | Runner | 工具链 |
|:---|:---|:---|
| x86_64-unknown-linux-gnu | ubuntu-latest | native |
| aarch64-unknown-linux-gnu | ubuntu-latest | cross（Docker）或 cargo-zigbuild |
| x86_64-pc-windows-msvc | windows-latest | native msvc |
| x86_64-apple-darwin | macos-13（Intel） | native |
| aarch64-apple-darwin | macos-14+（ARM） | native |

- CI：`matrix: [target]` 并行构建 → 产物上传 → tag 推送触发 Release（zip：二进制 + README + LICENSE + SHA-256）。**矩阵在 W3 即接入流水线**（非 W7 才做，风险前置）。
- macOS：Developer 证书签名 + notarytool 公证（未公证会被 Gatekeeper 拦截）；Windows：EV/OV 证书签名积累 SmartScreen 信誉。

## 10.4 版本管理

- SemVer `MAJOR.MINOR.PATCH`；破坏性数据迁移必须 MINOR+ 并在 Release Note 醒目标注（导入格式变更同此规则，附录 C）。
- 发版脚本 `scripts/release.sh`：工作区干净校验 → bump 版本（**Cargo.toml + frontend/package.json + tauri.conf.json 无关字段已移除**，修复 B3）→ 生成 CHANGELOG（git-cliff）→ 打 tag → push 触发 CI 矩阵构建。
- 应用内「关于」页展示应用版本、`schema_migrations` 最新版本、导出格式版本三元组，便于排障。

---

# 11. 测试策略

## 11.1 总体原则

- **LLM 是端口不是依赖**：`LlmProvider` trait + mockall Mock；引擎测试 100% 使用录制回放（真实响应样本脱敏入库），保证确定性回归。
- **覆盖率（务实版）**：核心业务逻辑（引擎/上下文/结构化校验/导出/回收站）行覆盖 ≥80%；handlers ≥70%；前端 utils/hooks ≥70%；不做全局硬指标，用 CRAP 指数把关复杂函数。
- **SQLite 真实行为**：集成测试一律 `tempfile` 真实文件库（覆盖 WAL/busy_timeout/FTS5 行为）；`:memory:` 仅限纯单元测试。

## 11.2 后端测试矩阵

| 层级 | 工具 | 内容 | 通过标准 |
|:---|:---|:---|:---|
| 单元 | cargo test | structured 校验/修复回路、director 三模式选角+防重复、情感限幅与夹紧、上下文压缩触发与 summary_until_seq 推进、**崩溃后上下文重建（§8.5 算法）**、配置加载优先级、迁移幂等、回收站级联/恢复重命名 | 全部通过 |
| 契约 | cargo test --test contract + export_bindings | DTO↔TS 类型新鲜度（git diff）；路由表与 endpoints 常量一致 | CI 失败即红 |
| 集成 | tokio + mockall + 临时库 | §6.2 全端点（含 404/409/422 错误路径）；事务回滚；**幂等并发竞争（场景 §8.8-2）**；SSE after_seq 重放与 Lagged 自愈；崩溃恢复例程 + recover 续跑；**abort 在 LLM 调用中即时取消（场景 §8.8-6）**；导入导出往返（附录 C）；回收站恢复/过期；FTS 同步（含恢复动作，修复 B10 验证） | 关键路径全覆盖 |
| 属性 | proptest | 迁移幂等（跑两遍=跑一遍）、情感值域不变式、序号唯一（软删后重建）、幂等回放一致性 | 10k cases 无失败 |

## 11.3 前端测试矩阵

| 层级 | 工具 | 内容 |
|:---|:---|:---|
| 单元 | Vitest + RTL | stores 归约、sse 封装重连/心跳/假死重建、错误码→文案映射表 |
| 组件 | Vitest + RTL + msw | 控制台三栏渲染（9 态按钮组）、ErrorBoundary 兜底、Toast 引导动作 |
| E2E | Playwright | 黄金路径：建项目→建角色→配羁绊→发起沙盒（Mock SSE 录制）→定稿导出→回收站恢复；断线重连（offline 1s）；双标签页同看（场景 §8.8-1） |

## 11.4 测试数据

- `tests/fixtures/recordings/*.json`：真实 LLM 响应录制（脱敏），Mock 按 `key = sha256(模板名+版本+角色ID+步号)` 回放（比 v3.0 的 prompt 哈希更稳定——模板渲染含时间性内容时哈希易碎）；录制文件版本化，模板升版同步重录。
- Mock Provider 另支持「脚本化响应」：测试用例内联定义响应序列（成功→非法 JSON→修复成功），覆盖修复回路。

---
# 12. 安全设计

## 12.1 网络面（修复 S8）

**本地服务双校验**（v4.0 新增）——无鉴权的 127.0.0.1 HTTP 服务面临两类现实攻击：

- **DNS rebinding**：攻击者域名先解析到其服务器（拿同源授权），TTL 后重绑到 127.0.0.1——此时恶意页面请求 `http://evil.com:8080/api/v1/novels` 实际命中本机服务，且同源策略放行，**可读走全部创作数据**。
- **跨站探测**：恶意页面以 `no-cors` 向常见本地端口发 POST（虽无法读响应，但可触发写操作副作用）。

**防护设计**：

```
HostGuard（全端点）：
  Host 头必须 ∈ {127.0.0.1:8080, localhost:8080, [::1]:8080}（按实际 bind 端口）
  否则 403 + warn 日志（含来源 UA）          ← 直接掐断 DNS rebinding（重绑域名 ≠ 白名单）

OriginGuard（非 GET/HEAD/HEAD 端点）：
  Origin 头存在时，必须匹配 http://127.0.0.1:8080 或 http://localhost:8080
  Origin 缺失（curl / 同源跳转）→ 放行        ← 现代浏览器跨站 fetch 必带 Origin
```

- **0.0.0.0 绑定模式**：启动时生成随机 32 字符令牌写入数据目录（0600）；首次访问经 `?token=` 引导，前端存 localStorage 后以 `X-App-Token` 头随请求携带；中间件校验失败 401。UI 常驻黄色警告条 + 「查看局域网访问者指南」。
- CORS：生产构建拒绝一切跨源；开发模式仅放行 `http://localhost:5173`。
- 请求体限额：默认 2MB，`/api/v1/import` 路由级 64MB（§5.4）；SSE 不压缩 + 心跳保活（§6.4）。
- Token 鉴权（可插拔 middleware）保留为 v4.x backlog：一旦提供公网服务形态，必须启用。

## 12.2 应用面

- **XSS**：所有 Markdown/用户文本渲染经 DOMPurify（白名单标签，禁 `iframe/script/on*`）；React 默认转义 + lint `react/no-danger` 禁用 `dangerouslySetInnerHTML`。
- **注入**：SQL 全参数化（sqlx），无字符串拼接；模板渲染 minijinja 沙箱特性；导出文件名白名单 `[A-Za-z0-9_\-一-龥]` + `Content-Disposition: attachment`。
- **SSRF / 出网白名单**：LLM base_url 变更时校验 host 属已知 Provider 白名单（deepseek/openai/anthropic/moonshot/ollama/自定义需二次确认）；私有网段（`169.254.169.254`、127/8、10/8、172.16/12、192.168/16）默认拒绝，ollama（`127.0.0.1:11434`）例外显式允许。
- **Prompt 注入威胁模型**（v4.0 新增，修复 B10 说明）：

```
信任边界：本机用户输入（角色卡/场景/指令）= 可信
          导入的第三方项目内容 = 半可信（进入 LLM 上下文，但不触达系统层）
LLM 输出  = 不可信输入（即使没有恶意，也可能是幻觉）
```

| 注入面 | 影响 | 缓解 |
|:---|:---|:---|
| 导入项目的角色卡含「忽略以上指令」类文本 | 沙盒行为跑偏（创作质量受损） | JSON Schema 约束输出结构；导入页提示「第三方内容将进入 LLM 上下文」 |
| LLM 输出含 HTML/script | XSS | actor_action.content 渲染经 DOMPurify（§12.2 第一条） |
| LLM 输出伪造 emotion_delta 巨值 | 数据污染 | Schema 限幅 ±20 + 写库夹紧（§8.6） |
| LLM 输出伪造 director_note 诱导下一步 | 导演被误导 | 导演只消费结构化字段与自有输入，director_note 仅展示不参与决策 |

结论：注入影响**限于创作质量**，不构成安全边界突破——本地单用户、无数据外泄通道（LLM 出网仅白名单 API）。

## 12.3 凭据与数据安全

- API Key 不以明文入库：`app_config(is_secret=1)` 存 AES-256-GCM 密文（Base64）；密钥经 HKDF 派生（OS 用户 SID + 安装随机盐，盐文件 0600）。**能力允许时优先系统 keyring**（`keyring` crate：Windows Credential Manager / macOS Keychain / Secret Service），文件加密为回退路径。
- 内存中密钥使用 `secrecy::SecretString`（零值化 drop），日志宏自动脱敏（`Display` 输出 `***`）。
- 数据库文件权限：Unix 0600、Windows ACL 仅当前用户；导出 zip 含明文设定（**不含密钥**），导出页明示「请自行妥善保管」。

## 12.4 依赖与供应链

- CI 强制 `cargo audit` 与 `npm audit --audit-level=high`，失败即红；Renovate 周级升级。
- 发布物附带 SHA-256 校验文件，下载页提供校验命令；CI 构建环境固定（rust-toolchain.toml + package-lock.json 双锁）。

## 12.5 日志隐私

- 结构化日志不落 Prompt 全文（默认仅哈希 + 前 64 字符）、不落密钥；`log.trace_prompt=true` 显式开启才记录全文，设置页开启时展示敏感警告。
- 诊断包（§14）默认脱敏；密钥永远掩码。

---

# 13. 性能与可靠性

## 13.1 性能预算（口径修正，修复 B2）

| 指标 | 预算 | 测量方式 |
|:---|:---|:---|
| 进程启动 → /api/healthz 200 | < 300ms | CI 基准（hyperfine） |
| 双击 → 浏览器可交互 | < 1.5s | 人工 + 录屏脚本 |
| 首页项目列表（100 项目） | < 150ms P95 | k6 本地压测 |
| 沙盒单步（含 LLM） | P95 < 4s，P99 < 8s | 录制回放 + 真实 Provider 抽样 |
| 空闲 / 4 会话运行内存 | < 120MB / < 350MB | CI 内存探针 |
| 单会话事件全量重放 | ≤300 条（step_limit 60 × 每步 ≤5 事件）< 100ms P95 | 集成测试计时 |
| 成本报表跨会话聚合 1 万条 llm_usage | < 500ms | 集成测试计时 |

- 性能回归门禁：CI 基准较基线劣化 >20% 警告、>50% 失败。
- 修正说明：v3.0「事件回放 1 万条」与单会话步数上限 60 自相矛盾；v4.0 拆为「单会话重放」（真实上限口径）与「跨会话聚合」（报表口径）。

## 13.2 可靠性设计汇总

| 故障 | 检测 | 应对 |
|:---|:---|:---|
| LLM 429/5xx/断网 | 重试中间件 + 故障转移链 | 同 Provider 限流等待 → 换 Provider → 模板降级（标记） |
| LLM 输出非法 JSON | Schema 校验 | 修复回路 ×2 → actor_error / 导演降级轮转 → 熔断 failed |
| 上下文超窗 | token 预估 | 滚动摘要 → 仍超硬顶才 failed（不留半态） |
| 进程崩溃 | 启动恢复例程 | 会话标 interrupted，recover 续跑（上下文重建算法 §8.5） |
| SQLite 锁竞争 | busy_timeout=5s | WAL + 单写排队；集成测试覆盖并发场景（§8.8-8） |
| SSE 断连 | 前端退避重连 | Last-Event-ID / after_seq 区间补发；Lagged DB 自愈 |
| 磁盘满 | 启动检查 + 写失败捕获 | 明确报错引导清理；日志轮替上限 100MB |
| 会话僵尸（进程内卡死） | 60s 扫描 | interrupted 标记，可 recover |
| 密钥解密失败（机器特征变化） | 启动自检 | 引导重置密钥流程（keyring 优先降低此风险） |

## 13.3 资源限额

- 日志：tracing-appender 日滚动，保留 7 天，总量上限 100MB。
- 单会话事件理论上限 = step_limit(≤60) × 每步事件数(≤5) = 300 条（**修正 v3.0「超过 5000 步强制 finalize」与 step_limit≤60 的直接矛盾**，修复 B1：不再需要步数兜底，控制台环形缓冲 500 条已覆盖全量；导出走 DB 全量）。
- 导出文件流式生成（tokio 流式写），大项目不占驻内存；导入 64MB 限额 + 事务内完成。

---

# 14. 运维与监控

- **单机运维定位**：无外部监控依赖；可观测性 = 结构化日志（JSON，含 request_id/session_id/step span）+ `/api/health` + 设置页「诊断」面板（最近错误、LLM 成功率与延迟分位、磁盘占用、一键打包诊断包）。
- **span 约定**：`session_id` / `step` / `provider` / `purpose` 贯穿引擎与 LLM 层；排障一条命令拉全链路：`jq 'select(.session_id=="…")' logs/*.json`。
- **诊断包**：日志（脱敏）+ schema_migrations + config（密钥掩码）+ 最近 7 天 llm_usage 聚合 + 系统信息，zip 导出供反馈；不包含 Prompt 全文（除非 trace_prompt 显式开启且用户勾选）。

---

# 15. 开发路线图（8 周）

| 阶段 | 周 | 交付物 | 验收标准（DoD） |
|:---|:---|:---|:---|
| P1 骨架 | W1-2 | 项目骨架、sqlx+SQLite+迁移框架、ts-rs 链路（export_bindings+CI 断言）、rust-embed、Host/Origin 中间件、novel/character/volume/chapter CRUD + 回收站全端点 | `cargo test` 全绿；契约测试通过；E2E 黄金路径骨架（建-改-删-恢复）打通；`APP_BIND=0.0.0.0` 启动横幅+令牌生效 |
| P2 引擎 | W3-4 | LlmProvider + 能力探测 + 故障转移 + 结构化校验修复回路、Director 三模式 + 防重复、SessionManager + hub + SSE（含重放/Lagged 自愈）、沙盒控制台、CI 三平台矩阵接入 | 录制回放集成测试全绿；§8.8 并发场景 1/2/5/6/7 用例通过；崩溃恢复演练（kill -9 后 recover）通过 |
| P3 完善 | W5-6 | 羁绊矩阵可视化、大纲编辑器、滚动摘要 + 上下文重建、成本报表、导入导出（附录 C）、设置页、全文检索、回收站自动过期 | Playwright 全场景通过；导入导出往返 + ID 重映射用例通过；FTS 同步（含恢复）用例通过；macOS 公证流程预演 |
| P4 发布 | W7-8 | 错误打磨、性能压测调优、签名/公证、文档与发版 | §13.1 性能预算达标；Release 五目标产物 + SHA-256；升级/回滚演练（备份恢复）通过 |

**关键路径**：P2 的「结构化校验 + 录制回放」是质量基线，必须在引擎功能膨胀前就位；Host/Origin 防护属 P1（安全左移）；羁绊可视化、全文检索可并行排期。

---

# 16. 风险登记册（修订版）

| 风险 | 概率 | 影响 | 应对 | 残余风险 |
|:---|:---|:---|:---|:---|
| LLM 输出不稳定导致沙盒跑飞 | 高 | 高 | JSON Schema + 修复回路 + 导演降级轮转 + 防重复硬上限 + actor_error 熔断 | 中：导演决策质量依赖模板调优 |
| 上下文超窗（长会话） | 高 | 中 | 滚动摘要 + summary_until_seq 边界 + 硬顶失败（不留半态） | 低 |
| SQLite 并发写瓶颈 | 中 | 中 | WAL + busy_timeout + 集成测试并发矩阵 | 低 |
| Provider 政策/价格/能力变动（如 json_object 语义变化） | 中 | 中 | Provider 抽象 + 能力探测 + 故障转移链；llm_pricing 可编辑 | 低 |
| UPX 触发杀软误报（Windows） | 中 | 中 | 提供非压缩版产物双轨发布；签名优先；文档说明加白名单 | 中：新签名信誉积累期 |
| 代理/企业网络环境 SSE 被缓冲或断连 | 中 | 中 | 心跳 15s + `X-Accel-Buffering: no` + Last-Event-ID 重放；文档列出已知代理配置 | 低 |
| Prompt 注入（导入第三方项目） | 低 | 低 | 威胁模型 §12.2：影响限于创作质量，无安全边界突破 | 低 |
| 用户 bind 0.0.0.0 暴露 | 低 | 高 | 默认 127.0.0.1 + Host 白名单 + 令牌强制 + 常驻警告 | 低（较 v3.0 收敛：令牌补齐） |
| TS 类型漂移 | 中 | 中 | export_bindings + CI git diff 断言 | 低 |
| 加密密钥机器特征变化 | 低 | 高 | keyring 优先 + 盐文件持久化 + 重置流程 | 低 |
| 交叉编译/签名阻塞发版 | 中 | 中 | 原生 runner 矩阵 W3 接入；公证 W5 预演 | 低 |

---
# 附录 A：v3.0 → v4.0 评审发现与修复对照（详尽证据版）

> 评审以「能否直接指导开发」为标准，对 v3.0 全文 17 章 + 2 附录逐项走查，覆盖架构完整性、数据一致性、并发与可靠性、LLM 工程、安全、构建发布、测试有效性、文档一致性八个维度。共发现 **34 项**：S 级 8（阻塞开发/技术错误）、A 级 15（设计断层）、B 级 11（一般缺陷）。每项含 v3.0 原文位置、问题与根因、影响、v4.0 修复去向四要素。

## A.1 严重问题（S 级，8 项）

| 编号 | v3.0 位置 | 问题与根因 | 影响 | v4.0 修复去向 |
|:---|:---|:---|:---|:---|
| S1 | 第一部分 §1.2–1.4 全部表格、附录 A、正文内部引用（§6.5→「§7.4」、§6.6→「§8.5」、§10.7→「§12.2」等） | **交叉引用系统性错位**：评审表与附录 A 的「修复位置」相对第二部分实际章节号整体偏移 1（如导演调度器实际 §9.2，表中写 §8.2；S5 指向 §7.4-7.6，实际 §8.4-8.6）。根因：插入「系统架构」章后全文章节号 +1，引用未同步 | 按图索骥找不到修复设计；评审承诺不可验证；v2.0→v3.0 的修复对照整体失真 | 本文档全文引用已重排对准；正文以「修复：Sx/Ax/Bx」角标显式标注 |
| S2 | §3.1「数据库」行、§5 章标题、§4.2 阻塞线程池段 | **libsql 与 sqlx 混用不成立**：sqlx 无 libsql 驱动，libsql crate 与 sqlx API 互不兼容，「libsql（Turso 本地文件）+ sqlx（sqlite flavor）」无法同时成立；「libsql 同步驱动需 spawn_blocking 隔离」与两库实际（均异步）不符 | 数据层在开发第一周即被迫临场选型，连接/PRAGMA/迁移设计全部返工 | ADR-06（选型修正 + 回退路径）；§3.1/§3.2/§4.5 统一为 sqlx + bundled SQLite |
| S3 | §5.1 characters / items 表定义；chapters 表 novel_id | **SQLite DDL 语法错误**：表内 `UNIQUE (novel_id, name) WHERE deleted_at IS NULL` 不是合法表约束（部分唯一必须 `CREATE UNIQUE INDEX … WHERE`），基线迁移执行即报错；chapters 的 novel_id 外键声明两次（行内 REFERENCES + 表级 FOREIGN KEY） | 首次启动迁移失败；DDL 无法作为开发依据 | §4.1 全部改为部分唯一索引；chapters 单 FK + novel_id 受控冗余说明 |
| S4 | §5.1 volumes `UNIQUE(novel_id,volume_index)`、chapters `UNIQUE(volume_id,chapter_index)` | **软删除与唯一约束冲突**：全量唯一约束包含已软删行——软删第 3 卷后该序号被永久占位，无法重建；与 B2「软删除/回收站」承诺直接矛盾 | 回收站不可用，或被迫改硬删丢数据 | §4.1 部分唯一索引（WHERE deleted_at IS NULL）；§4.7 恢复时序号/名称冲突策略 |
| S5 | §9.4 SessionHandle 定义与其后事件双写/多端 bullet | **SSE 事件通道自相矛盾**：同段先定义 `event_tx: mpsc::Sender`（容量 256、队列满阻塞=背压），又称 SSE 层「从 event_tx 的 broadcast::Receiver（容量 64）读取」——mpsc 单消费者无法支撑多端同看；broadcast 无背压；「引擎阻塞在 send」会反压落盘与 LLM 步进 | 事件通道实现必然偏离文档，「多端观看」与「背压」二选一，设计意图不明 | §8.4 hub 模式：落盘事务为唯一背压点，broadcast 仅扇出；Lagged 走 DB 区间补发自愈 |
| S6 | §8.5 rolling_summary 字段、§9.5 崩溃恢复/续跑 | **摘要覆盖边界缺失**：有 rolling_summary 但无「摘要覆盖至哪个 seq」字段，恢复时无法区分已压缩/未压缩事件，上下文重建算法不成立 | recover 无法正确实现：上下文要么超窗要么丢剧情 | §4.1 `summary_until_seq` 列；§7.5 压缩算法；§8.5 重建算法（摘要 + tail 原文）闭环 |
| S7 | §7.1（build.rs 生成 TS 类型）、§11.1/11.2（build.rs 自动执行） | **build.rs 内调用 cargo 反模式**：cargo 构建期持有 target 目录构建锁，build.rs 再执行 `cargo test`（ts-rs 生成）存在死锁/递归构建风险；ts-rs 官方路径是测试导出 | 构建系统偶发锁死；类型生成链路不可靠 | ADR-07；§6.1 `cargo test --test export_bindings` 唯一生成入口 + CI git diff 新鲜度断言；build.rs 仅嵌入与存在性校验 |
| S8 | §13.1 网络面（仅有 bind 提示与 CORS） | **本地服务 Host/Origin 校验缺失**：无鉴权 127.0.0.1 服务面临 DNS rebinding（重绑域名绕过同源策略读取全部创作数据）与跨站 no-cors 探测两类现实攻击；标准防护（Host 白名单 + Origin 校验）缺失 | 用户浏览任意恶意网页期间，本机数据可被读取、写操作可被诱导触发 | ADR-08；§12.1 HostGuard + OriginGuard；0.0.0.0 模式强制启动令牌 |

## A.2 重要问题（A 级，15 项）

| 编号 | v3.0 位置 | 问题与根因 | 影响 | v4.0 修复去向 |
|:---|:---|:---|:---|:---|
| A1 | §9.2 模式 B JSON 示例 vs §9.2 终止条件 2、§9.3 状态机 | 导演输出 Schema 与终止/等待机制脱节：终止条件引用 `{"terminate":true}`、状态机含 waiting_input，但 Schema 示例仅 next_actor_id/reason/inject_scene_hint——两个状态无产生入口 | finalize 与 waiting_input 两大机制不可达 | §7.4 Director Schema 补 terminate/request_user_input；§8.2 分支消费；§8.3 迁移表 |
| A2 | §9.3 状态机（run 迁移）vs §7.2 control 动作表 | created→running 触发（自动/手动）未定义；control 无 start；step 单步语义（源状态/执行后状态）未定义 | 会话是否自动运行两可；单步调试无法实现 | §6.3 autostart；§6.2 六动作；§8.3 迁移表（step: paused→paused 恰好一步） |
| A3 | §8.5 与 §8.7 均标「迁移 0003」 | 迁移编号冲突：rolling_summary 与 template_versions 两个不同变更都声称 0003；且 v3.0 为未发布新版本，这两列本应进 0001 基线 | 迁移顺序歧义；首次安装结构依赖执行路径 | §4.1/§4.2 全部入 0001 基线（v3.0 未发布无存量库） |
| A4 | §7.2 端点表 vs §10.3 路由表 | 按项目列会话端点缺失：前端 `/novel/:id/sandbox` loader 预取 sessions，但端点表仅有按章节创建与按 sid 查询 | 沙盒列表页无数据源 | §6.2 `GET /api/v1/novels/{id}/sandbox-sessions` |
| A5 | §7.2（POST /novels 幂等）vs §5.1 表结构 | novels 幂等承诺无存储支撑：表无幂等字段、无通用幂等记录表 | 端点承诺与实现不符 | §4.1 idempotency_records；§5.5 通用幂等中间件（novels/sessions/import） |
| A6 | §5.4 search_index 定义 | FTS5 元数据列（entity_type/entity_id/novel_id）作为普通全文列被分词——搜索 "chapter" 命中所有章节行 | 检索精度差、索引浪费 | §4.1 UNINDEXED 列 |
| A7 | §6.4 中间件链 vs §7.2 import 端点 | 导入与全局 BodyLimit(2MB) 冲突（大项目导出 JSON 轻松超限，导入必 413）；CompressionLayer 未排除 SSE 路由（缓冲破坏实时性） | 大项目无法导入；SSE 实时性在压缩/代理组合下丧失 | §5.4 分路由限额（import 64MB）；SSE 路由排除压缩 |
| A8 | §9.5「续跑」vs §7.2 control 动作 | interrupted 恢复动作缺失：control 无 recover；并发闸许可、上下文重建流程均未设计 | 「崩溃后 100% 可恢复」目标（§2.2）无法兑现 | §8.3 recover 迁移；§8.5 恢复算法（重建/新 deadline/续流） |
| A9 | §8.2 超时表 vs 附录 B（llm_timeout_seconds=60）vs §6.2（步超时 90s） | 超时体系冲突：全局 60s、步 90s、重试 ×2 叠加关系未定义（最坏 270s）；「全会话超时」列为终止条件但无配置无语义 | 超时行为不可预测；长会话可能无限占用并发闸 | §7.2 三层预算体系；session_deadline 落库；waiting_input 超时 |
| A10 | §10.2（引用 outlineStore）vs §10.1（stores 目录） | 前端 store 清单与跨 store 示例不一致：引用了不存在的 outlineStore | 工程结构歧义；示例不可落地 | §9.1 补 outlineStore（卷/章大纲树） |
| A11 | §10.5（错误边界自动上报）vs §7.2 | client-errors 上报端点未列入 API 契约 | 前端异常上报 404 | §6.2 POST /api/v1/client-errors（32KB/写日志/204） |
| A12 | §5.1（deleted_at）+ B2 承诺 vs 全文 | 回收站有软删无恢复/清除：无列表/恢复/永久清除端点、无级联清理规则、无过期策略 | 「误删可恢复」空转；软删行累积 | §4.7 全设计；§6.2 三端点；自动过期任务 |
| A13 | §8.4「json_schema（DeepSeek/OpenAI 均支持）」 | DeepSeek 能力描述不实：json_schema 为 OpenAI 系能力，DeepSeek 当前仅 json_object，按文档直连默认 Provider 首调即 400 | 默认配置不可用 | §7.4 StructuredMode 能力探测 + json_object 降级（Schema 摘要入 prompt + 本地校验） |
| A14 | §6.6 僵尸扫描（仅 running）与「每步结束后检查令牌」 | 扫描遗漏 finalizing 态（卡死永不清理）；abort 在 LLM 调用期间最坏等 90s | 僵尸 finalizing 永久占位；中止体验差 | §5.6 扫描含 finalizing/paused 超期；select! 即时取消（半步不落盘） |
| A15 | §9.2 模式 B | 防重复选角无规则：导演决策协议无连续选角上限，LLM 导演可能锁死单角色 | 群像失衡、产出单一视角 | §8.2 软提示（模板注入连续步数）+ 硬上限 director_repeat_limit + 降级事件 |

## A.3 一般问题（B 级，11 项）

| 编号 | v3.0 位置 | 问题 | v4.0 修复去向 |
|:---|:---|:---|:---|
| B1 | §14.3「超过 5000 步强制 finalize」 | 与 step_limit ≤ 60 直接矛盾（永不触达） | §13.3 理论上限推导（60×5=300 条）+ 环形缓冲覆盖 |
| B2 | §14.1「事件回放 1 万条 <500ms」 | 指标口径不实：单会话事件远少于 1 万 | §13.1 拆「单会话重放 ≤300 条 <100ms」与「跨会话聚合 1 万条 <500ms」 |
| B3 | §11.4 release.sh「tauri 无关字段」 | 非 Tauri 项目残留字段 | §10.4 已移除 |
| B4 | §11.3 cross 编译全部 5 目标 | cross 不支持 x86_64-pc-windows-msvc | §10.3 原生 runner 矩阵 |
| B5 | §5.1 prompt_templates | 缺「同名仅一条激活」约束 | §4.1 uq_prompt_active 部分唯一索引 |
| B6 | §5.1/§5.6 llm_usage | 缺 (session_id) 索引，会话快照 usage 聚合全表扫 | §4.1 idx_usage_session |
| B7 | §8.6「成本单价表内维护」 | 未指明哪张表/何种维护方式 | §7.6 app_config 键 llm_pricing（JSON、设置页可编辑） |
| B8 | §7.2（名称唯一大小写不敏感）vs §5.1 UNIQUE(novel_id,name) | 约束大小写敏感与 API 约定不一致 | §4.1 表达式索引 lower(name) |
| B9 | §5.1 llm_usage 仅 success 0/1 | 失败无分类，排障困难 | §4.1 error_kind 列 |
| B10 | §5.4「写操作后同事务更新 FTS」 | 同步点不完整：遗漏恢复动作的索引重建 | §4.4 四同步点 + §11.2 测试覆盖 |
| B11 | 附录 B vs 正文 | 心跳间隔、Retry-After、硬顶等多个运行参数散落正文、未入配置表 | 附录 B 全量收编（新增 15 键） |

## A.4 评审结论

v3.0 相对 v2.0 有实质进步（导演层抽象、会话状态机、幂等意识、成本追踪均已出现），但存在三类系统性问题：**其一，一致性失守**（S1 引用错位、S3/S4 DDL 与约束错误、A3 迁移冲突）——文档自身承诺与正文脱节，说明成文后缺少一次全文交叉校验；**其二，机制半途**（S5/S6/A1/A8/A9）——事件通道、摘要边界、恢复动作等关键机制写到一半，恰是开发中期最易返工的位置；**其三，外部事实未核实**（S2 驱动组合、A13 Provider 能力、B4 工具能力）。v4.0 已逐项修复并以角标回链，同时补齐模板全文、事件全集、迁移表、时序图、并发矩阵等落地细节，达到「按文档可直接开工」标准。

---

# 附录 B：关键配置项速查

| 配置键 | 默认 | 说明 | 热更新 |
|:---|:---|:---|:---:|
| bind | 127.0.0.1:8080 | 监听地址（0.0.0.0 强制令牌，§12.1） | 否 |
| data_dir | ""（平台默认） | 数据目录 | 否 |
| llm.provider / llm.model | deepseek / deepseek-chat | 主 Provider | 是 |
| llm.base_url | api.deepseek.com/v1 | 出网白名单校验（§12.2） | 是 |
| llm.fallbacks | [] | 故障转移链，如 ["openai:gpt-4o-mini"] | 是 |
| llm.connect_timeout_seconds | 5 | 连接超时 | 是 |
| llm.attempt_timeout_seconds | 45 | 单次尝试上限（§7.2 预算体系） | 是 |
| llm.max_retries | 2 | 单次尝试链内瞬时错误重试 | 是 |
| sandbox.max_concurrent | 4 | 全局沙盒并发闸（创建与 recover 共用） | 是 |
| sandbox.max_steps | 20 | 会话默认步数上限（创建可覆盖 1..=60） | 是 |
| sandbox.step_timeout_seconds | 90 | 步预算 | 是 |
| sandbox.session_timeout_seconds | 1800 | 会话预算（session_deadline） | 是 |
| sandbox.history_max_tokens | 6000 | 滚动摘要触发阈值 | 是 |
| sandbox.context_hard_cap_tokens | 12000 | 上下文硬顶（超限 failed） | 是 |
| sandbox.rolling_summary_max_tokens | 500 | 摘要上限 | 是 |
| sandbox.director_repeat_limit | 3 | 同角色连续行动硬上限 | 是 |
| sandbox.waiting_input_timeout_seconds | 300 | 等待输入超时（超时温和收束） | 是 |
| http.body_limit_bytes | 2097152 | 默认请求体限额 | 否 |
| http.import_body_limit_bytes | 67108864 | 导入专用限额 | 否 |
| http.sse_heartbeat_seconds | 15 | SSE 心跳间隔 | 是 |
| recycle.retention_days | 30 | 回收站保留天数 | 是 |
| cost.monthly_budget_usd | 0.0 | 月度预算（0=不限；超限新会话 429） | 是 |
| log.level | info | 日志级别 | 是 |
| log.trace_prompt | false | 记录 Prompt 全文（敏感，§12.5） | 是 |
| fts_min_query_length | 2 | 检索最小词长 | 是 |

> 环境变量前缀 `APP_`（如 `APP_LLM_API_KEY`）；热更新经设置页 PATCH `/api/v1/config` 白名单生效（§6.2）。v4.0 新增 15 键（修复 B11）。

---
# 附录 C：导入导出格式规范（v4.0 新增）

## C.1 包结构

```
{novel_title}-{yyyymmdd-HHmmss}.saexport   （zip）
├── manifest.json          ← 元数据与校验（导入先读此文件）
└── data.json              ← 全量数据（单 JSON，流式生成）
```

## C.2 manifest.json 字段定义

```json
{
  "format_version": 2,
  "schema_version": 4,
  "app_version": "4.0.0",
  "exported_at": 1791187200,
  "novel": { "id": "…", "title": "…", "summary": "…" },
  "counts": {
    "volumes": 3, "chapters": 45, "characters": 12,
    "relations": 30, "items": 8,
    "sessions": 10, "events": 1520, "llm_usage": 0
  },
  "checksums": { "data.json": "sha256:…" },
  "options": { "include_sessions": true, "include_deleted": false }
}
```

| 字段 | 类型 | 说明 |
|:---|:---|:---|
| format_version | int | 导出格式版本（当前 2；v3.0 未实际发布导出，不存在 format_version=1 存量包） |
| schema_version | int | 导出时的库结构版本（对齐 schema_migrations 最新值） |
| app_version | string | 导出端应用版本（SemVer） |
| counts | object | 实体计数（导入后对账校验依据） |
| checksums | object | data.json 的 SHA-256（防传输损坏） |
| options | object | include_sessions=是否含沙盒历史；include_deleted=是否含回收站内容 |

## C.3 导入流程（事务内，失败全回滚）

```
1. 解包 → 校验 checksums（不匹配 → 422 ValidationFailed）
2. format_version > 当前支持 → 422 UnsupportedImportVersion（引导升级应用）
3. format_version < 当前 → 字段级兼容映射（映射表随格式版本演进维护于 export_service）
4. ID 重映射（防与库中现有数据冲突）：
   导入实体全部生成新 ID；构建 旧ID→新ID 映射表；
   依次重写引用：chapters.volume_id、relations.char_a/b_id、items.owner_id、
   sessions.chapter_id/actor_ids、events.session_id
5. 唯一性预处理：名称与现有冲突时按 §4.7 恢复策略（追加「（导入）」）；
   卷/章序号整体平移不重排
6. 单事务写入全部实体 + FTS 重建 + llm_usage 不导入（历史成本留在原机器）
7. 返回导入报告 {novel_id, imported: counts, renamed: [...], skipped: [...]}
```

## C.4 兼容矩阵

| 导出 format_version | 导入端支持 | 处理 |
|:---|:---|:---|
| 2 | v4.0+ | 直接导入 |
| >2 | 任意旧版本 | 422 UnsupportedImportVersion |
| 未来 3 | v4.x 追加 | 新增字段须可缺省（向前兼容纪律：只增可选字段，不改语义） |

- **纪律**：format_version 变更必须 MINOR+ 发版并在 Release Note 标注（§10.4）；导出永远使用「当前版本可表达的最低格式」以最大化互通。

---

# 附录 D：术语表

| 术语 | 定义 |
|:---|:---|
| Director（导演） | 沙盒调度层：决定「谁在下一步行动」，不生成内容；三模式 roundrobin/director/scripted（§8.2） |
| Actor（演员/角色执行器） | 沙盒内容生成层：代入单个角色产生一次行动（对话/动作/内心/移动）+ 情感增量 |
| Step（步） | 一次「导演选角 + 演员行动 + 情感应用 + 落盘」的完整循环；步预算 90s |
| Session（会话） | 一次沙盒演进的全过程，含九态状态机（§8.3）与独立事件流 |
| rolling_summary（前情摘要） | 被压缩的历史摘要（≤500 token），代表 seq ≤ summary_until_seq 的全部事件 |
| summary_until_seq | 摘要覆盖边界：上下文重建时分隔「摘要区」与「原文区」的 seq（§7.5/§8.5） |
| 步预算 / 会话预算 | 三层超时预算体系（§7.2）：attempt 45s ⊂ step 90s ⊂ session 1800s |
| hub（事件总线） | 每会话的 broadcast 通道；落盘事务为唯一背压点，Lagged 走 DB 区间自愈（§8.4） |
| seq | 会话内单调递增事件序号；同时是 SSE `id:` 与重放游标（§6.4） |
| 修复回路 | 结构化输出校验失败后，将原始输出与错误清单回喂 LLM 要求修正（最多 2 轮，§7.4） |
| 模板降级 | Provider 全链失败后的本地占位大纲生成（带 ⚠️ 标记，§7.3） |
| 能力探测 | Provider 对 json_schema / json_object response_format 的支持判定（§7.4） |
| 幂等键 | Idempotency-Key 头 + idempotency_records 表；同键同体重放、同键异体 409（§5.5） |
| 回收站 | deleted_at 软删除实体集合；恢复/永久清除/30 天自动过期（§4.7） |
| HostGuard / OriginGuard | 本地服务防 DNS rebinding 与跨站请求的双校验中间件（§12.1） |
| 启动令牌 | 0.0.0.0 绑定模式强制启用的 X-App-Token 校验（§12.1） |
| FTS5 | SQLite 全文检索扩展；元数据列 UNINDEXED（§4.4） |
| 部分唯一索引 | `CREATE UNIQUE INDEX … WHERE deleted_at IS NULL`：唯一性仅约束未删除行（§4.1） |
| recover | interrupted 会话的恢复动作：闸许可 + 上下文重建 + 续流（§8.5） |
| 单步执行（step） | paused 态执行恰好一步后回到 paused 的调试语义（§8.3） |
| DoD | Definition of Done，阶段验收标准（§15） |
| ts-rs 测试导出 | `cargo test --test export_bindings` 触发的 TS 类型生成模式（ADR-07） |
| golden path（黄金路径） | E2E 主链路：建项目→建角色→配羁绊→沙盒→定稿→导出（§11.3） |

---
*AI生成*
