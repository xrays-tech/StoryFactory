-- 0008_novel_style.sql — 小说级文风档案（风格名/范文/额外要求/默认叙述人称）
ALTER TABLE novels ADD COLUMN style_name TEXT;
ALTER TABLE novels ADD COLUMN style_sample TEXT;
ALTER TABLE novels ADD COLUMN style_instruction TEXT;
ALTER TABLE novels ADD COLUMN pov TEXT;
