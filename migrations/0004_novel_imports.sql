-- 0004_novel_imports.sql —— txt 小说上传与逆向分析任务
-- 任务状态由后台 runner 推进；analysis_json 保存 NovelImportPreview 的内部形状。
CREATE TABLE IF NOT EXISTS novel_imports (
  id           TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  filename     TEXT NOT NULL DEFAULT '',
  size_bytes   INTEGER NOT NULL DEFAULT 0,
  raw_text     TEXT NOT NULL,
  status       TEXT NOT NULL DEFAULT 'queued'
               CHECK (status IN ('queued','analyzing','ready','failed','cancelled')),
  stage        TEXT NOT NULL DEFAULT 'parsed',
  progress     INTEGER NOT NULL DEFAULT 0,
  chunks_total INTEGER NOT NULL DEFAULT 0,
  chunks_done  INTEGER NOT NULL DEFAULT 0,
  message      TEXT,
  error        TEXT,
  analysis_json TEXT,
  novel_id     TEXT REFERENCES novels(id) ON DELETE SET NULL,
  created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
  updated_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_novel_imports_created ON novel_imports(created_at DESC);
