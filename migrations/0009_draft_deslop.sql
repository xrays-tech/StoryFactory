-- 0009_draft_deslop.sql — 正文去 AI 味：任务开关与 lint 报告
ALTER TABLE chapter_drafts ADD COLUMN deslop INTEGER;
ALTER TABLE chapter_drafts ADD COLUMN lint_json TEXT;
