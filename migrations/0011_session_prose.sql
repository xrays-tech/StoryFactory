-- 0011_session_prose.sql —— 会话收束后自动生成正文
ALTER TABLE sandbox_sessions ADD COLUMN prose_draft_id TEXT;
-- 收束后是否自动生成正文：1 开启（默认），0 关闭。随会话持久化，恢复/续跑时仍遵守用户选择。
ALTER TABLE sandbox_sessions ADD COLUMN auto_prose INTEGER NOT NULL DEFAULT 1;
