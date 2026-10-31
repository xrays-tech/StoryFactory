-- 0005_chapter_beats.sql —— 章节情节清单与沙盒消费进度
-- chapters.beats：JSON 字符串数组，声明本章应按顺序演绎的关键情节。
-- sandbox_sessions.beats_done：JSON 整数数组，记录已被演绎的情节下标。
ALTER TABLE chapters ADD COLUMN beats TEXT;
ALTER TABLE sandbox_sessions ADD COLUMN beats_done TEXT;
