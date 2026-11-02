-- 0010_chapter_target_words.sql — 章节目标生成字数（默认 3500 字）
ALTER TABLE chapters ADD COLUMN target_words INTEGER NOT NULL DEFAULT 3500;
