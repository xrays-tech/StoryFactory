-- ============================================================
-- 0001_init.sql —— v4.0 完整基线（§4.1，D-25: sqlx migrate 管理 `_sqlx_migrations`，
--   自建 schema_migrations 表删除；PRAGMA user_version 同步写入由 db.rs）
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
  deleted_at  INTEGER,                       -- 软删除时间戳，NULL=正常
  deleted_batch TEXT                         -- D-11: 级联恢复批次 UUID，NULL=未删
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
  deleted_at    INTEGER,
  deleted_batch TEXT                         -- D-11
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
  sandbox_log   TEXT,                        -- 沙盒完整日志（JSON，导出用；保留列，当前版本不写入 D-27）
  final_outline TEXT,                        -- 定稿大纲（Markdown）
  status        TEXT NOT NULL DEFAULT 'draft'
                CHECK (status IN ('draft','outlined','completed','archived')),
  created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  deleted_at    INTEGER,
  deleted_batch TEXT                         -- D-11
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
  deleted_at         INTEGER,
  deleted_batch      TEXT                      -- D-11
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_characters_name
  ON characters(novel_id, lower(name)) WHERE deleted_at IS NULL;  -- 大小写不敏感（修复 B8）
CREATE INDEX IF NOT EXISTS idx_characters_novel
  ON characters(novel_id) WHERE deleted_at IS NULL;

-- ========== 角色羁绊矩阵 ==========
-- 行规范化：写入前强制 char_a_id < char_b_id（UPSERT 归一化）。
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
  deleted_at  INTEGER,
  deleted_batch TEXT                         -- D-11
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
  summary_until_seq INTEGER NOT NULL DEFAULT 0,   -- 摘要覆盖边界
  rolling_summary   TEXT,                         -- 前情摘要（≤500 token）
  pending_directive TEXT,                       -- D-14: waiting_input 唤醒指令（崩溃保留）
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
  purpose           TEXT NOT NULL,            -- actor_step / director / summary / repair / finalize（D-06）
  prompt_tokens     INTEGER NOT NULL,
  completion_tokens INTEGER NOT NULL,
  cost_usd          REAL NOT NULL DEFAULT 0,
  latency_ms        INTEGER NOT NULL,
  success           INTEGER NOT NULL DEFAULT 1,
  error_kind        TEXT,                     -- 失败分类（修复 B9）
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
  response_body TEXT NOT NULL,                -- 成功响应快照
  replay_hint   INTEGER NOT NULL DEFAULT 0,   -- D-04: 响应 >16KB 时存摘要并标记
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
-- D-22: trigram 分词（CJK 前序修复）；元数据列 UNINDEXED（修复 A6）
CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
  title, body,
  entity_type UNINDEXED, entity_id UNINDEXED, novel_id UNINDEXED,
  tokenize = 'trigram'
);

-- D-25: 迁移记录由 sqlx migrate 内部表 `_sqlx_migrations`；自建 schema_migrations 表删除。
