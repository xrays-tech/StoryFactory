-- 0006_chapter_prose.sql —— 章节正文与正文生成草稿
ALTER TABLE chapters ADD COLUMN content TEXT;         -- 正文
ALTER TABLE chapters ADD COLUMN word_count INTEGER;   -- 正文字数（字符数）
CREATE TABLE IF NOT EXISTS chapter_drafts (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  novel_id TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
  chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
  status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','generating','ready','failed','cancelled')),
  stage TEXT, progress INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
  style_name TEXT, style_sample TEXT, instruction TEXT,
  target_words INTEGER NOT NULL DEFAULT 2000,
  content TEXT, message TEXT, error TEXT,
  created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_chapter_drafts_chapter ON chapter_drafts(chapter_id, created_at DESC);
